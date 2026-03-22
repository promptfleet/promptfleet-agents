//! LLM orchestration over the protocol-free engine.
//!
//! This module provides runtime orchestration plus compatibility wrappers:
//! - `crate::a2a::execute_a2a`: adapter-owned request-response compatibility entry point
//! - [`run_tools_loop_stream`]: native-only streaming entry point with trace events
//! - [`run_tools_loop_agnostic`]: protocol-agnostic streaming entry point
//!
//! The core tool-calling loop lives in [`engine::core_loop`](super::engine::core_loop).
//! LLM invoker traits and policy configuration are in [`llm_invoker`](super::llm_invoker)
//! and re-exported here for backward compatibility.
//! Sentinel-tool finalization logic is in [`finalization`](super::finalization).

// ── Re-exports ──────────────────────────────────────────────────────────
//
// All public items from `llm_invoker` are re-exported so that existing
// import paths (`agent_sdk::agent::llm_orchestrator::LlmPolicy` etc.)
// continue to work unchanged.
pub use super::llm_invoker::*;

// ── Finalization helpers (crate-internal) ───────────────────────────────
use super::finalization::{build_response_from_finalization_args, run_finalization_turn};

// ── Other imports ───────────────────────────────────────────────────────
use crate::agent::history_policy::HistoryPolicyRuntime;
use crate::agent::tools::ToolRegistry;
use crate::agent::{MessageContext, TaskContext};
use crate::agent::{Response, RuntimeResponse, TaskOpts};
use crate::error::SdkResult;
use agent_core::{ContentPart, TaskPhase};
use log::debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// All utility helpers (apply_request_defaults, build_tools_json, context-window
// management) now live in engine::core_loop — the single source of truth.

// =========================================================================
// Native-only: streaming tools loop over ToolEngine core
// =========================================================================

/// Execute a streaming tools loop that yields [`AgentTraceEvent`]s in
/// real-time.
///
/// This is the streaming counterpart of [`run_tools_loop`]. It consumes
/// an [`LlmStreamInvoker`] and yields trace events through a channel as
/// the LLM generates tokens and tools are executed.
///
/// The returned [`AgentTraceStream`] completes when the agent finishes
/// (either with [`AgentTraceEvent::Completed`] or [`Failed`]).
///
/// # Implementation
///
/// This is a thin runtime wrapper that:
/// 1. Builds OpenAI-format messages from `MessageContext` / `TaskContext`
/// 2. Converts `LlmPolicy` to `EngineConfig`
/// 3. Delegates to [`engine::core_loop::execute`] — the single source of truth
///
/// # Differences from `run_tools_loop`
///
/// - Uses streaming LLM calls (real SSE from the provider)
/// - Emits `ContentDelta` / `ReasoningDelta` per-token
/// - Emits `ToolCallStarted` / `ToolCallCompleted` around each tool execution
/// - Does **not** handle `checkpoint_task` finalization
/// - Native-only (requires tokio runtime)
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tools_loop_stream(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
) -> crate::agent::trace::AgentTraceStream {
    run_tools_loop_stream_with_cancel(
        llm,
        model,
        tools,
        policy,
        msg_ctx,
        task_ctx,
        system_message,
        request_defaults,
        None,
    )
}

/// Same as [`run_tools_loop_stream`] but shares a cancellation flag with the
/// caller and flips it automatically if the returned stream is dropped.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tools_loop_stream_with_cancel(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> crate::agent::trace::AgentTraceStream {
    run_tools_loop_stream_with_skills_and_history_runtime(
        llm,
        model,
        tools,
        policy,
        msg_ctx,
        task_ctx,
        system_message,
        request_defaults,
        None,
        None,
        cancel_flag,
        None,
        crate::agent::history_policy::default_runtime(),
    )
}

/// Streaming tools loop with optional skill context injection.
///
/// Same as [`run_tools_loop_stream`] but also injects resolved skill context
/// and a skill summary into the message list before the engine starts.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tools_loop_stream_with_skills(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
    skill_context: Option<crate::agent::skill::SkillContext>,
    skill_summary: Option<String>,
    cancel_flag: Option<Arc<AtomicBool>>,
    request_headers: Option<Arc<std::collections::HashMap<String, String>>>,
) -> crate::agent::trace::AgentTraceStream {
    run_tools_loop_stream_with_skills_and_history_runtime(
        llm,
        model,
        tools,
        policy,
        msg_ctx,
        task_ctx,
        system_message,
        request_defaults,
        skill_context,
        skill_summary,
        cancel_flag,
        request_headers,
        crate::agent::history_policy::default_runtime(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_tools_loop_stream_with_skills_and_history_runtime(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
    skill_context: Option<crate::agent::skill::SkillContext>,
    skill_summary: Option<String>,
    cancel_flag: Option<Arc<AtomicBool>>,
    request_headers: Option<Arc<std::collections::HashMap<String, String>>>,
    history_policy_runtime: Arc<dyn HistoryPolicyRuntime>,
) -> crate::agent::trace::AgentTraceStream {
    use crate::agent::engine::{core_loop, StreamingTurnInvoker};
    use crate::agent::trace::AgentTraceEvent;

    let (tx, rx) = tokio::sync::mpsc::channel::<AgentTraceEvent>(64);
    let tx_for_delta = tx.clone();
    let tx_for_tools = tx.clone();
    let cancel_flag = cancel_flag.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let cancel_flag_for_task = cancel_flag.clone();

    let delta_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
        let _ = tx_for_delta.try_send(event);
    });
    let invoker = StreamingTurnInvoker::new(llm, delta_sink);

    tokio::spawn(async move {
        let prepared_history = match task_ctx.as_ref() {
            Some(task_ctx) => Some(
                history_policy_runtime
                    .prepare_turn(task_ctx, &msg_ctx, system_message.as_deref())
                    .await,
            ),
            None => None,
        };
        let (effective_system_message, retained_history) = match prepared_history {
            Some(Ok(prepared)) => (prepared.system_message, Some(prepared.retained_history)),
            Some(Err(err)) => {
                let _ = tx.try_send(AgentTraceEvent::Failed {
                    message: err.to_string(),
                });
                return;
            }
            None => (system_message.clone(), None),
        };
        let mut messages = build_runtime_messages_with_history(
            &msg_ctx,
            retained_history
                .as_deref()
                .or_else(|| task_ctx.as_ref().map(|ctx| ctx.runtime_history.as_slice()))
                .unwrap_or(&[]),
            effective_system_message.as_deref(),
        );

        if let Some(ref ctx) = skill_context {
            crate::agent::skill::inject_skill_context(&mut messages, ctx);
        }
        if let Some(ref summary) = skill_summary {
            inject_system_supplement(&mut messages, summary);
        }

        let config = policy_to_engine_config(&policy, request_defaults.as_ref());

        let on_event = |event: AgentTraceEvent| {
            let _ = tx.try_send(event);
        };
        let event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
            let _ = tx_for_tools.try_send(event);
        });
        let _ = core_loop::execute(
            &invoker,
            &model,
            &tools,
            &config,
            &mut messages,
            &on_event,
            Some(event_sink),
            Some(cancel_flag_for_task),
            request_headers,
        )
        .await;
    });

    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    Box::pin(futures::stream::unfold(
        (rx, CancelOnDrop(cancel_flag)),
        |(mut rx, guard)| async move { rx.recv().await.map(|item| (item, (rx, guard))) },
    ))
}

/// **Protocol-agnostic entry point** for the streaming tools loop.
///
/// Accepts `agent_core` types so callers don't need
/// to import protocol-specific crates. Internally it builds runtime contexts and
/// delegates to [`run_tools_loop_stream`].
///
/// # Arguments
///
/// * `input` - Protocol-agnostic user message (text, data, or mixed)
/// * `history` - Optional conversation context with prior turns
///
/// All other arguments are the same as [`run_tools_loop_stream`].
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tools_loop_agnostic(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    input: agent_core::AgentMessage,
    history: Option<agent_core::ConversationContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
) -> crate::agent::trace::AgentTraceStream {
    run_tools_loop_agnostic_with_cancel(
        llm,
        model,
        tools,
        policy,
        input,
        history,
        system_message,
        request_defaults,
        None,
        None,
    )
}

/// Same as [`run_tools_loop_agnostic`] but shares a cancellation flag with the
/// caller and flips it if the returned stream is dropped.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tools_loop_agnostic_with_cancel(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    input: agent_core::AgentMessage,
    history: Option<agent_core::ConversationContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
    cancel_flag: Option<Arc<AtomicBool>>,
    request_headers: Option<Arc<std::collections::HashMap<String, String>>>,
) -> crate::agent::trace::AgentTraceStream {
    run_tools_loop_agnostic_with_cancel_and_history_runtime(
        llm,
        model,
        tools,
        policy,
        input,
        history,
        system_message,
        request_defaults,
        cancel_flag,
        request_headers,
        crate::agent::history_policy::default_runtime(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_tools_loop_agnostic_with_cancel_and_history_runtime(
    llm: Arc<dyn LlmStreamInvoker>,
    model: String,
    tools: ToolRegistry,
    policy: LlmPolicy,
    input: agent_core::AgentMessage,
    history: Option<agent_core::ConversationContext>,
    system_message: Option<String>,
    request_defaults: Option<LlmRequestDefaults>,
    cancel_flag: Option<Arc<AtomicBool>>,
    request_headers: Option<Arc<std::collections::HashMap<String, String>>>,
    history_policy_runtime: Arc<dyn HistoryPolicyRuntime>,
) -> crate::agent::trace::AgentTraceStream {
    let msg_ctx = MessageContext::from_agent_message(input);
    let task_ctx = history.map(TaskContext::from_conversation);
    run_tools_loop_stream_with_skills_and_history_runtime(
        llm,
        model,
        tools,
        policy,
        msg_ctx,
        task_ctx,
        system_message,
        request_defaults,
        None,
        None,
        cancel_flag,
        request_headers,
        history_policy_runtime,
    )
}

// =========================================================================
// Runtime helpers: message building + config conversion
// =========================================================================

/// Append text to the system message (or create one if absent).
fn inject_system_supplement(messages: &mut Vec<serde_json::Value>, supplement: &str) {
    if let Some(first) = messages.first_mut() {
        if first.get("role").and_then(|r| r.as_str()) == Some("system") {
            if let Some(content) = first.get("content").and_then(|c| c.as_str()) {
                *first = serde_json::json!({
                    "role": "system",
                    "content": format!("{}{}", content, supplement)
                });
                return;
            }
        }
    }
    messages.insert(
        0,
        serde_json::json!({
            "role": "system",
            "content": supplement.trim_start()
        }),
    );
}

/// Build provider messages from runtime-first `MessageContext` and `TaskContext`.
pub(crate) fn build_runtime_messages(
    msg_ctx: &MessageContext,
    task_ctx: Option<&TaskContext>,
    system_message: Option<&str>,
) -> Vec<serde_json::Value> {
    let history = task_ctx
        .map(|ctx| ctx.runtime_history.as_slice())
        .unwrap_or(&[]);
    build_runtime_messages_with_history(msg_ctx, history, system_message)
}

pub(crate) fn build_runtime_messages_with_history(
    msg_ctx: &MessageContext,
    history: &[agent_core::AgentMessage],
    system_message: Option<&str>,
) -> Vec<serde_json::Value> {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    if let Some(sys) = system_message {
        if !sys.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": sys}));
        }
    }

    for m in history {
        let role = match m.role {
            agent_core::Role::User => "user",
            agent_core::Role::Agent => "assistant",
            agent_core::Role::System => "system",
        };
        let mut text_content = String::new();
        for p in &m.parts {
            if let agent_core::ContentPart::Text(t) = p {
                if !text_content.is_empty() {
                    text_content.push('\n');
                }
                text_content.push_str(t);
            }
        }
        if !text_content.is_empty() {
            messages.push(serde_json::json!({"role": role, "content": text_content}));
        }
    }

    let user_text = msg_ctx
        .text_content
        .clone()
        .unwrap_or_else(|| msg_ctx.runtime_message.text_content().unwrap_or_default());
    let combined_user = match render_runtime_data_parts(&msg_ctx.runtime_message) {
        Some(dctx) if !dctx.is_empty() => format!("{}\n\nUser: {}", dctx, user_text),
        _ => user_text,
    };
    messages.push(serde_json::json!({"role": "user", "content": combined_user}));

    messages
}

fn render_runtime_data_parts(message: &agent_core::AgentMessage) -> Option<String> {
    let mut chunks = Vec::new();
    for part in &message.parts {
        if let agent_core::ContentPart::Data(value) = part {
            if let Ok(json) = serde_json::to_string_pretty(value) {
                chunks.push(json);
            }
        }
    }

    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

/// Convert an `LlmPolicy` to an `EngineConfig`.
pub(crate) fn policy_to_engine_config(
    policy: &LlmPolicy,
    request_defaults: Option<&LlmRequestDefaults>,
) -> crate::agent::engine::EngineConfig {
    crate::agent::engine::EngineConfig {
        max_turns: policy.max_turns.unwrap_or(10),
        max_tool_calls: policy.max_tool_calls,
        wall_clock_timeout_ms: policy.wall_clock_timeout_ms,
        max_context_tokens: policy.max_context_tokens,
        request_defaults: request_defaults.cloned(),
    }
}

// =========================================================================
// Runtime execution (replaces the legacy request-response loop)
// =========================================================================

/// Execute the tool-calling loop via the universal engine and wrap the
/// result as a protocol-neutral [`RuntimeResponse`].
///
/// This is the engine-backed runtime entrypoint. It:
/// 1. Builds provider messages from runtime contexts
/// 2. Converts `LlmPolicy` → `EngineConfig`
/// 3. Creates a `RequestResponseTurnInvoker` wrapping the `LlmInvoker`
/// 4. Calls `engine::core_loop::execute()`
/// 5. Interprets stop signals (checkpoint_task) and safety-gate errors
///    to produce the correct runtime response
pub(crate) async fn execute_runtime(
    llm: Arc<dyn LlmInvoker>,
    model: &str,
    tools: &ToolRegistry,
    policy: &LlmPolicy,
    msg_ctx: &MessageContext,
    task_ctx: Option<TaskContext>,
    system_message: Option<&str>,
    request_defaults: Option<&LlmRequestDefaults>,
    skill_context: Option<&crate::agent::skill::SkillContext>,
    skill_summary: Option<&str>,
    history_policy_runtime: &dyn HistoryPolicyRuntime,
) -> SdkResult<RuntimeResponse> {
    use crate::agent::engine::{core_loop, RequestResponseTurnInvoker};

    let original_task_ctx = task_ctx.clone();
    let prepared_history = match task_ctx.as_ref() {
        Some(task_ctx) => Some(
            history_policy_runtime
                .prepare_turn(task_ctx, msg_ctx, system_message)
                .await?,
        ),
        None => None,
    };
    let prepared_system = prepared_history
        .as_ref()
        .and_then(|prepared| prepared.system_message.as_deref())
        .or(system_message);
    let prepared_history_slice = prepared_history
        .as_ref()
        .map(|prepared| prepared.retained_history.as_slice())
        .or_else(|| task_ctx.as_ref().map(|ctx| ctx.runtime_history.as_slice()))
        .unwrap_or(&[]);

    let mut messages =
        build_runtime_messages_with_history(msg_ctx, prepared_history_slice, prepared_system);

    // Inject resolved skill context (instructions + handler output) into messages
    if let Some(ctx) = skill_context {
        crate::agent::skill::inject_skill_context(&mut messages, ctx);
    }

    // Inject skill summary so the LLM knows about read_skill capabilities
    if let Some(summary) = skill_summary {
        inject_system_supplement(&mut messages, summary);
    }

    let config = policy_to_engine_config(policy, request_defaults);
    let invoker = RequestResponseTurnInvoker::new(llm.clone());

    let result = core_loop::execute(
        &invoker,
        model,
        tools,
        &config,
        &mut messages,
        &|_| {},
        None,
        None,
        None,
    )
    .await;

    match result {
        Ok(engine_result) => {
            if let Some(ref stop_signal) = engine_result.stop_signal {
                // Sentinel tool (checkpoint_task) signalled stop — extract args
                if let Some(args) = stop_signal.get("__checkpoint_args") {
                    let response = build_response_from_finalization_args(
                        args.clone(),
                        msg_ctx,
                        task_ctx.clone(),
                        policy,
                    )?;
                    return attach_continuation_update(
                        history_policy_runtime,
                        original_task_ctx.as_ref(),
                        msg_ctx,
                        prepared_system,
                        response,
                    )
                    .await;
                }
            }

            // Normal text response → completed task
            let text = engine_result.text.unwrap_or_default();
            if text.is_empty() {
                // No text and no stop signal — empty response
                attach_continuation_update(
                    history_policy_runtime,
                    original_task_ctx.as_ref(),
                    msg_ctx,
                    prepared_system,
                    Response::task(
                        TaskOpts {
                            state: Some(TaskPhase::Completed),
                            ..Default::default()
                        },
                        msg_ctx,
                        task_ctx.clone(),
                    )?,
                )
                .await
            } else if policy.finalize_required {
                // Policy requires structured finalization — force a checkpoint turn
                let post = "Model returned text without calling checkpoint_task; produce a structured checkpoint_task response.";
                if let Ok(args) =
                    run_finalization_turn(llm.clone(), model, tools, messages, post).await
                {
                    let response = build_response_from_finalization_args(
                        args,
                        msg_ctx,
                        task_ctx.clone(),
                        policy,
                    )?;
                    return attach_continuation_update(
                        history_policy_runtime,
                        original_task_ctx.as_ref(),
                        msg_ctx,
                        prepared_system,
                        response,
                    )
                    .await;
                }
                // Fallback: use the text directly
                attach_continuation_update(
                    history_policy_runtime,
                    original_task_ctx.as_ref(),
                    msg_ctx,
                    prepared_system,
                    Response::task(
                        TaskOpts {
                            state: Some(TaskPhase::Completed),
                            history_parts: Some(vec![ContentPart::Text(text)]),
                            ..Default::default()
                        },
                        msg_ctx,
                        task_ctx.clone(),
                    )?,
                )
                .await
            } else {
                attach_continuation_update(
                    history_policy_runtime,
                    original_task_ctx.as_ref(),
                    msg_ctx,
                    prepared_system,
                    Response::task(
                        TaskOpts {
                            state: Some(TaskPhase::Completed),
                            history_parts: Some(vec![ContentPart::Text(text)]),
                            ..Default::default()
                        },
                        msg_ctx,
                        task_ctx.clone(),
                    )?,
                )
                .await
            }
        }
        Err(engine_err) => {
            let reason = engine_err.to_string();
            debug!("execute_runtime: engine error: {}", reason);

            if policy.finalize_required {
                let post = format!("Stop reason: {}", reason);
                match run_finalization_turn(llm.clone(), model, tools, messages, &post).await {
                    Ok(args) => {
                        let response = build_response_from_finalization_args(
                            args,
                            msg_ctx,
                            task_ctx.clone(),
                            policy,
                        )?;
                        attach_continuation_update(
                            history_policy_runtime,
                            original_task_ctx.as_ref(),
                            msg_ctx,
                            prepared_system,
                            response,
                        )
                        .await
                    }
                    Err(_) => {
                        attach_continuation_update(
                            history_policy_runtime,
                            original_task_ctx.as_ref(),
                            msg_ctx,
                            prepared_system,
                            Response::task(
                                TaskOpts {
                                    state: Some(TaskPhase::Failed),
                                    status_text: Some(reason.clone()),
                                    history_parts: Some(vec![ContentPart::Text(reason)]),
                                    ..Default::default()
                                },
                                msg_ctx,
                                task_ctx.clone(),
                            )?,
                        )
                        .await
                    }
                }
            } else {
                attach_continuation_update(
                    history_policy_runtime,
                    original_task_ctx.as_ref(),
                    msg_ctx,
                    prepared_system,
                    Response::task(
                        TaskOpts {
                            state: Some(TaskPhase::Failed),
                            status_text: Some(reason.clone()),
                            history_parts: Some(vec![ContentPart::Text(reason)]),
                            ..Default::default()
                        },
                        msg_ctx,
                        task_ctx,
                    )?,
                )
                .await
            }
        }
    }
}

async fn attach_continuation_update(
    history_policy_runtime: &dyn HistoryPolicyRuntime,
    task_ctx: Option<&TaskContext>,
    msg_ctx: &MessageContext,
    system_message: Option<&str>,
    response: RuntimeResponse,
) -> SdkResult<RuntimeResponse> {
    let update = history_policy_runtime
        .complete_turn(task_ctx, msg_ctx, &response, system_message)
        .await?;

    match (response, update) {
        (RuntimeResponse::Task(mut runtime), Some(update)) => {
            runtime.continuation_update = Some(crate::agent::response::RuntimeContinuationUpdate {
                strategy_kind: update.strategy.kind,
                strategy_version: update.strategy.version,
                strategy_composition: update.strategy.composition,
                payload: update.payload,
            });
            Ok(RuntimeResponse::Task(runtime))
        }
        (response, _) => Ok(response),
    }
}

// ===========================================================================
// Tests: adapter compatibility finalization/checkpoint behavior
// ===========================================================================

#[cfg(all(test, not(target_arch = "wasm32")))]
mod adapter_compat_tests {
    use super::*;
    use crate::a2a::{MessageRole, MessageSendResponse, Task, TaskState};
    use crate::agent::tools::{ToolExecutor, ToolSpec};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct MockRequestInvoker {
        responses: Mutex<VecDeque<Result<serde_json::Value, String>>>,
        calls: AtomicUsize,
    }

    impl MockRequestInvoker {
        fn new(responses: Vec<Result<serde_json::Value, String>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: AtomicUsize::new(0),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl LlmInvoker for MockRequestInvoker {
        fn request(
            &self,
            _payload: serde_json::Value,
        ) -> std::pin::Pin<
            Box<dyn core::future::Future<Output = Result<serde_json::Value, String>> + Send>,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let next = self
                .responses
                .lock()
                .expect("mock invoker lock poisoned")
                .pop_front()
                .unwrap_or_else(|| Err("no mock response queued".to_string()));
            Box::pin(async move { next })
        }
    }

    fn checkpoint_tools() -> ToolRegistry {
        let mut tools = ToolRegistry::new();
        tools.register(ToolSpec {
            name: "checkpoint_task".to_string(),
            description: Some("Sentinel checkpoint tool".to_string()),
            parameters: json!({"type":"object"}),
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    Ok(json!({
                        "__engine_stop": true,
                        "__checkpoint_args": args,
                    }))
                })
            })),
        });
        tools
    }

    fn test_msg_ctx(text: &str) -> MessageContext {
        MessageContext::from_runtime_message(
            agent_core::AgentMessage::new(
                agent_core::Role::User,
                vec![agent_core::ContentPart::Text(text.to_string())],
            ),
            Default::default(),
            Default::default(),
            false,
            None,
        )
    }

    fn extract_last_agent_text(task: &Task) -> Option<String> {
        if let Some(message) = &task.status.message {
            let text = message.get_text_content();
            if !text.is_empty() {
                return Some(text);
            }
        }

        task.history.as_ref().and_then(|history| {
            history
                .iter()
                .rev()
                .find(|m| m.role == MessageRole::Agent)
                .map(|m| m.get_text_content())
        })
    }

    fn checkpoint_tool_call_response(args: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_ck",
                        "type": "function",
                        "function": {
                            "name": "checkpoint_task",
                            "arguments": args.to_string()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })
    }

    fn text_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": { "content": content },
                "finish_reason": "stop"
            }]
        })
    }

    #[tokio::test]
    async fn execute_a2a_uses_stop_signal_checkpoint_args_directly() {
        let invoker = Arc::new(MockRequestInvoker::new(vec![Ok(
            checkpoint_tool_call_response(json!({
                "task_patch": {
                    "state": "completed",
                    "append_history_text": "structured done"
                },
                "respond": { "kind": "task" }
            })),
        )]));
        let llm: Arc<dyn LlmInvoker> = invoker.clone();

        let result = crate::a2a::execute_a2a(
            llm,
            "gpt-4o-mini",
            &checkpoint_tools(),
            &LlmPolicy::default(),
            &test_msg_ctx("hello"),
            None,
            Some("system"),
            None,
            None,
            None,
        )
        .await
        .expect("execute_a2a should succeed");

        match result {
            MessageSendResponse::Task(task) => {
                assert_eq!(task.status.state, TaskState::Completed);
                assert_eq!(
                    extract_last_agent_text(&task).as_deref(),
                    Some("structured done")
                );
            }
            other => panic!("expected Task response, got {:?}", other),
        }
        assert_eq!(
            invoker.call_count(),
            1,
            "should not need fallback checkpoint turn"
        );
    }

    #[tokio::test]
    async fn execute_a2a_finalize_required_runs_forced_checkpoint_turn() {
        let invoker = Arc::new(MockRequestInvoker::new(vec![
            Ok(text_response("plain text that should be superseded")),
            Ok(checkpoint_tool_call_response(json!({
                "task_patch": {
                    "state": "completed",
                    "append_history_text": "finalized via checkpoint"
                },
                "respond": { "kind": "task" }
            }))),
        ]));
        let llm: Arc<dyn LlmInvoker> = invoker.clone();

        let result = crate::a2a::execute_a2a(
            llm,
            "gpt-5",
            &checkpoint_tools(),
            &LlmPolicy::default(),
            &test_msg_ctx("summarize"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("execute_a2a should succeed");

        match result {
            MessageSendResponse::Task(task) => {
                assert_eq!(task.status.state, TaskState::Completed);
                assert_eq!(
                    extract_last_agent_text(&task).as_deref(),
                    Some("finalized via checkpoint")
                );
            }
            other => panic!("expected Task response, got {:?}", other),
        }
        assert_eq!(
            invoker.call_count(),
            2,
            "should run one extra checkpoint turn"
        );
    }

    #[tokio::test]
    async fn execute_a2a_finalize_required_falls_back_to_plain_text_when_checkpoint_not_called() {
        let invoker = Arc::new(MockRequestInvoker::new(vec![
            Ok(text_response("plain fallback text")),
            Ok(text_response("still no checkpoint tool call")),
        ]));
        let llm: Arc<dyn LlmInvoker> = invoker.clone();

        let result = crate::a2a::execute_a2a(
            llm,
            "gpt-4o-mini",
            &checkpoint_tools(),
            &LlmPolicy::default(),
            &test_msg_ctx("fallback"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("execute_a2a should still succeed via fallback");

        match result {
            MessageSendResponse::Task(task) => {
                assert_eq!(task.status.state, TaskState::Completed);
                assert_eq!(
                    extract_last_agent_text(&task).as_deref(),
                    Some("plain fallback text")
                );
            }
            other => panic!("expected Task response, got {:?}", other),
        }
        assert_eq!(invoker.call_count(), 2);
    }

    #[tokio::test]
    async fn execute_a2a_finalize_required_on_error_uses_checkpoint_turn_then_failed_state() {
        let invoker = Arc::new(MockRequestInvoker::new(vec![
            Err("upstream 503".to_string()),
            Ok(checkpoint_tool_call_response(json!({
                "task_patch": {
                    "state": "failed",
                    "status_text": "structured failure reason"
                },
                "respond": { "kind": "task" }
            }))),
        ]));
        let llm: Arc<dyn LlmInvoker> = invoker.clone();

        let result = crate::a2a::execute_a2a(
            llm,
            "gpt-4o-mini",
            &checkpoint_tools(),
            &LlmPolicy::default(),
            &test_msg_ctx("trigger error"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("execute_a2a should recover through checkpoint turn");

        match result {
            MessageSendResponse::Task(task) => {
                assert_eq!(task.status.state, TaskState::Failed);
                let status_text = task
                    .status
                    .message
                    .as_ref()
                    .map(|m| m.get_text_content())
                    .unwrap_or_default();
                assert_eq!(status_text, "structured failure reason");
            }
            other => panic!("expected Task response, got {:?}", other),
        }
        assert_eq!(invoker.call_count(), 2);
    }
}

// ===========================================================================
// Tests: streaming tools loop
// ===========================================================================

#[cfg(all(test, not(target_arch = "wasm32")))]
mod stream_tests {
    use super::*;
    use crate::agent::tools::{ToolExecutor, ToolSpec};
    use crate::agent::trace::AgentTraceEvent;
    use futures::StreamExt;
    use llm_client::model_client::ClientError;
    use std::sync::Arc;

    struct MockStreamInvoker {
        turns: std::sync::Mutex<Vec<Vec<llm_client::StreamEvent>>>,
    }

    impl MockStreamInvoker {
        fn new(turns: Vec<Vec<llm_client::StreamEvent>>) -> Self {
            Self {
                turns: std::sync::Mutex::new(turns),
            }
        }
    }

    impl LlmStreamInvoker for MockStreamInvoker {
        fn request_stream(&self, _payload: serde_json::Value) -> LlmStreamFuture {
            let events = {
                let mut guard = self.turns.lock().unwrap();
                if guard.is_empty() {
                    Vec::new()
                } else {
                    guard.remove(0)
                }
            };
            Box::pin(async move {
                let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ClientError>));
                Ok(Box::pin(stream) as llm_client::LlmEventStream)
            })
        }
    }

    fn scenario_invoker(turns: Vec<Vec<llm_client::StreamEvent>>) -> Arc<dyn LlmStreamInvoker> {
        Arc::new(MockStreamInvoker::new(turns))
    }

    /// Build a minimal `MessageContext` for testing
    fn test_msg_ctx(text: &str) -> MessageContext {
        MessageContext::from_runtime_message(
            agent_core::AgentMessage::new(
                agent_core::Role::User,
                vec![agent_core::ContentPart::Text(text.to_string())],
            ),
            Default::default(),
            Default::default(),
            false,
            None,
        )
    }

    /// Build a `ToolRegistry` with a simple echo tool
    fn test_tools() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(ToolSpec {
            name: "echo".to_string(),
            description: Some("Echo the input".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    Ok(serde_json::json!({"echoed": args}))
                })
            })),
        });
        reg
    }

    // ── Test: simple text response ─────────────────────────────────────

    #[tokio::test]
    async fn stream_text_response() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: Some("resp-1".into()),
                model: Some("gpt-4".into()),
            },
            llm_client::StreamEvent::ContentDelta {
                delta: "Hello".into(),
            },
            llm_client::StreamEvent::ContentDelta {
                delta: " world".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: Some(llm_client::Usage {
                    prompt_tokens: Some(5),
                    completion_tokens: Some(2),
                    total_tokens: Some(7),
                }),
            },
        ]]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("Hi"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // Expected: TurnStarted, ContentDelta, ContentDelta, TurnCompleted, Completed
        assert_eq!(events.len(), 5, "events: {:#?}", events);

        assert!(matches!(
            &events[0],
            AgentTraceEvent::TurnStarted { turn: 1, .. }
        ));
        assert!(matches!(&events[1], AgentTraceEvent::ContentDelta { delta } if delta == "Hello"));
        assert!(matches!(&events[2], AgentTraceEvent::ContentDelta { delta } if delta == " world"));
        assert!(
            matches!(&events[3], AgentTraceEvent::TurnCompleted { turn: 1, finish_reason } if finish_reason.as_deref() == Some("stop"))
        );

        match &events[4] {
            AgentTraceEvent::Completed { text, usage } => {
                assert_eq!(text.as_deref(), Some("Hello world"));
                let u = usage.as_ref().unwrap();
                assert_eq!(u.total_tokens, Some(7));
            }
            other => panic!("expected Completed, got {:?}", other),
        }
    }

    // ── Test: tool call → text response ────────────────────────────────

    #[tokio::test]
    async fn stream_tool_call_then_text() {
        let invoker = scenario_invoker(vec![
            // Turn 1: LLM calls echo tool
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: Some("resp-1".into()),
                    model: Some("gpt-4".into()),
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_abc".into(),
                    name: "echo".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\"".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: ":\"hello\"}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // Turn 2: LLM responds with text after seeing tool result
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: Some("resp-2".into()),
                    model: Some("gpt-4".into()),
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "Done!".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("echo hello"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // Expected sequence (streaming path):
        // TurnStarted(1), ToolCallStarted(Null), ToolCallArgsDelta×2,
        // ToolCallArgsCompleted, ToolCallCompleted, TurnCompleted(1),
        // TurnStarted(2), ContentDelta, TurnCompleted(2), Completed
        assert_eq!(events.len(), 11, "events: {:#?}", events);

        assert!(matches!(
            &events[0],
            AgentTraceEvent::TurnStarted { turn: 1, .. }
        ));
        assert!(
            matches!(&events[1], AgentTraceEvent::ToolCallStarted { name, arguments, .. }
            if name == "echo" && arguments.is_null())
        );
        assert!(matches!(
            &events[2],
            AgentTraceEvent::ToolCallArgsDelta { .. }
        ));
        assert!(matches!(
            &events[3],
            AgentTraceEvent::ToolCallArgsDelta { .. }
        ));
        match &events[4] {
            AgentTraceEvent::ToolCallArgsCompleted {
                name, arguments, ..
            } => {
                assert_eq!(name, "echo");
                assert_eq!(arguments["text"], "hello");
            }
            other => panic!("expected ToolCallArgsCompleted, got {:?}", other),
        }
        match &events[5] {
            AgentTraceEvent::ToolCallCompleted {
                name,
                success,
                result,
                ..
            } => {
                assert_eq!(name, "echo");
                assert!(success);
                assert_eq!(result["echoed"]["text"], "hello");
            }
            other => panic!("expected ToolCallCompleted, got {:?}", other),
        }
        assert!(matches!(
            &events[6],
            AgentTraceEvent::TurnCompleted { turn: 1, .. }
        ));

        assert!(matches!(
            &events[7],
            AgentTraceEvent::TurnStarted { turn: 2, .. }
        ));
        assert!(matches!(&events[8], AgentTraceEvent::ContentDelta { delta } if delta == "Done!"));
        assert!(matches!(
            &events[9],
            AgentTraceEvent::TurnCompleted { turn: 2, .. }
        ));
        assert!(
            matches!(&events[10], AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("Done!"))
        );
    }

    // ── Test: reasoning deltas ─────────────────────────────────────────

    #[tokio::test]
    async fn stream_reasoning_deltas() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: Some("resp-1".into()),
                model: Some("qwen3-235b".into()),
            },
            llm_client::StreamEvent::ReasoningDelta {
                delta: "Let me think...".into(),
            },
            llm_client::StreamEvent::ReasoningDelta {
                delta: " The answer is clear.".into(),
            },
            llm_client::StreamEvent::ContentDelta { delta: "42".into() },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "qwen3-235b".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("What is the answer?"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // TurnStarted, ReasoningStarted, ReasoningDelta, ReasoningDelta,
        // ReasoningCompleted, ContentDelta, TurnCompleted, Completed
        assert_eq!(events.len(), 8, "events: {:#?}", events);

        assert!(matches!(
            &events[0],
            AgentTraceEvent::TurnStarted { turn: 1, .. }
        ));
        assert!(matches!(
            &events[1],
            AgentTraceEvent::ReasoningStarted { .. }
        ));
        assert!(
            matches!(&events[2], AgentTraceEvent::ReasoningDelta { delta } if delta == "Let me think...")
        );
        assert!(
            matches!(&events[3], AgentTraceEvent::ReasoningDelta { delta } if delta == " The answer is clear.")
        );
        assert!(matches!(
            &events[4],
            AgentTraceEvent::ReasoningCompleted { .. }
        ));
        assert!(matches!(&events[5], AgentTraceEvent::ContentDelta { delta } if delta == "42"));
        assert!(
            matches!(&events[7], AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("42"))
        );
    }

    // ── Test: turn limit triggers Failed ───────────────────────────────

    #[tokio::test]
    async fn stream_turn_limit() {
        // The invoker always returns a tool call, so the loop should hit the turn limit
        let invoker = scenario_invoker(vec![
            // Turn 1
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c1".into(),
                    name: "echo".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // Turn 2 (will exceed max_turns=1)
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "done".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut policy = LlmPolicy::default();
        policy.max_turns = Some(1);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            policy,
            test_msg_ctx("test"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // After turn 1 completes + tool execution, turn 2 should be blocked by max_turns=1
        let last = events.last().unwrap();
        assert!(
            matches!(last, AgentTraceEvent::Failed { message } if message.contains("Turn limit")),
            "expected Failed with turn limit, got {:?}",
            last
        );
    }

    // ── Test: failed tool call ─────────────────────────────────────────

    #[tokio::test]
    async fn stream_failed_tool() {
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c1".into(),
                    name: "nonexistent".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // After failed tool, LLM returns text
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "Sorry".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("call nonexistent"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // Find the ToolCallCompleted with success=false
        let tool_completed = events
            .iter()
            .find(|e| matches!(e, AgentTraceEvent::ToolCallCompleted { success: false, .. }));
        assert!(
            tool_completed.is_some(),
            "expected a failed ToolCallCompleted, events: {:#?}",
            events
        );

        // Should still complete after the failed tool (LLM responds on next turn)
        let last = events.last().unwrap();
        assert!(
            matches!(last, AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("Sorry")),
            "expected Completed, got {:?}",
            last
        );
    }

    // ── Test: many small argument fragments assembled correctly ─────────

    #[tokio::test]
    async fn stream_tool_call_many_arg_fragments() {
        // Simulate realistic fine-grained argument streaming: each token
        // is a separate ToolCallDelta with a tiny JSON fragment
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_frag".into(),
                    name: "echo".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "\"te".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "xt\"".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: ":".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "\"hel".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "lo w".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "orld\"".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // After tool execution, LLM finishes
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta { delta: "ok".into() },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("test"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // ToolCallStarted is now emitted early with Null args (AG-UI announcement).
        // Verify via ToolCallArgsCompleted which holds the fully assembled arguments.
        let completed_args = events.iter().find_map(|e| match e {
            AgentTraceEvent::ToolCallArgsCompleted {
                id,
                name,
                arguments,
                ..
            } => Some((id.clone(), name.clone(), arguments.clone())),
            _ => None,
        });
        let (id, name, args) = completed_args.expect("expected ToolCallArgsCompleted event");
        assert_eq!(id, "call_frag");
        assert_eq!(name, "echo");
        assert_eq!(
            args["text"], "hello world",
            "fragments should assemble into valid JSON; got {:?}",
            args
        );

        // Verify tool was actually called with the assembled args
        let completed = events.iter().find_map(|e| match e {
            AgentTraceEvent::ToolCallCompleted {
                name,
                result,
                success,
                ..
            } => Some((name.clone(), result.clone(), *success)),
            _ => None,
        });
        let (cname, cresult, csuccess) = completed.expect("expected ToolCallCompleted event");
        assert_eq!(cname, "echo");
        assert!(csuccess);
        assert_eq!(cresult["echoed"]["text"], "hello world");
    }

    // ── Test: parallel tool calls in a single turn ─────────────────────

    #[tokio::test]
    async fn stream_parallel_tool_calls() {
        // Two tool calls interleaved in the same turn (OpenAI sends them
        // as interleaved ToolCallStart/ToolCallDelta with different indices)
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                // First tool call header
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_a".into(),
                    name: "echo".into(),
                },
                // Second tool call header
                llm_client::StreamEvent::ToolCallStart {
                    index: 1,
                    id: "call_b".into(),
                    name: "echo".into(),
                },
                // Interleaved argument deltas
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\"".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 1,
                    arguments_delta: "{\"text\"".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: ":\"alpha\"}".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 1,
                    arguments_delta: ":\"beta\"}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // After both tools, LLM finishes
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "both done".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("test"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // ToolCallStarted is now emitted early with Null args.
        // Verify assembled arguments via ToolCallArgsCompleted.
        let args_completed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AgentTraceEvent::ToolCallArgsCompleted {
                    index,
                    id,
                    arguments,
                    ..
                } => Some((*index, id.clone(), arguments.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(
            args_completed.len(),
            2,
            "expected 2 ToolCallArgsCompleted, got {:?}",
            args_completed
        );

        assert_eq!(args_completed[0].0, 0);
        assert_eq!(args_completed[0].1, "call_a");
        assert_eq!(args_completed[0].2["text"], "alpha");

        assert_eq!(args_completed[1].0, 1);
        assert_eq!(args_completed[1].1, "call_b");
        assert_eq!(args_completed[1].2["text"], "beta");

        // Both should have completed successfully
        let completed: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                AgentTraceEvent::ToolCallCompleted {
                    index,
                    id,
                    success,
                    result,
                    ..
                } => Some((*index, id.clone(), *success, result.clone())),
                _ => None,
            })
            .collect();

        assert_eq!(completed.len(), 2);
        assert!(completed[0].2, "call_a should succeed");
        assert_eq!(completed[0].3["echoed"]["text"], "alpha");
        assert!(completed[1].2, "call_b should succeed");
        assert_eq!(completed[1].3["echoed"]["text"], "beta");

        // Final output
        let last = events.last().unwrap();
        assert!(
            matches!(last, AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("both done")),
            "expected Completed, got {:?}",
            last
        );
    }

    // ── Test: malformed JSON arguments fall back to _raw ───────────────

    #[tokio::test]
    async fn stream_tool_call_malformed_args() {
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_bad".into(),
                    name: "echo".into(),
                },
                // Intentionally malformed JSON
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "not valid json{".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            // LLM follows up after seeing tool result
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "recovered".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let mut stream = run_tools_loop_stream(
            invoker,
            "gpt-4".into(),
            test_tools(),
            LlmPolicy::default(),
            test_msg_ctx("test"),
            None,
            None,
            None,
        );

        let mut events: Vec<AgentTraceEvent> = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // ToolCallArgsCompleted should contain the _raw fallback for malformed JSON
        let args_completed = events
            .iter()
            .find_map(|e| match e {
                AgentTraceEvent::ToolCallArgsCompleted { arguments, .. } => Some(arguments.clone()),
                _ => None,
            })
            .expect("expected ToolCallArgsCompleted");

        assert!(
            args_completed.get("_raw").is_some(),
            "malformed args should produce _raw fallback, got {:?}",
            args_completed
        );
        assert_eq!(args_completed["_raw"], "not valid json{");
    }
}
