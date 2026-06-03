//! Core tool-calling execution loop — the single source of truth.
//!
//! This module contains the protocol-agnostic inner loop that drives
//! LLM-to-tool-call iterations. It operates on [`llm_client::ChatMessage`]
//! and emits [`AgentTraceEvent`]s through a callback.
//!
//! No A2A types, no `MessageContext`, no `TaskContext`, no `checkpoint_task`.
//! Works on both WASM (via `RequestResponseTurnInvoker`) and native
//! (via `StreamingTurnInvoker`).

use crate::agent::llm_invoker::LlmRequestDefaults;
use crate::agent::tool_context::ToolContext;
use crate::agent::tools::ToolRegistry;
use crate::agent::trace::AgentTraceEvent;
use llm_client::{ChatContentPart, ChatMessage, LlmRequest, ToolChoice, ToolSchema};
use log::debug;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::types::{EngineConfig, EngineError, EngineResult, LlmTurnInvoker};

// =========================================================================
// Public entry point
// =========================================================================

/// Run the tool-calling loop on pre-built messages.
///
/// Emits [`AgentTraceEvent`]s through `on_event` as the loop progresses.
/// Returns an [`EngineResult`] on success, or a typed [`EngineError`].
///
/// The `messages` vec is mutated in-place (tool results appended) so that
/// the caller can inspect the final conversation state on error — useful
/// for adapters that need to run a forced finalization turn.
///
/// This function is the canonical implementation — all SDK entry points
/// (streaming, text, compatibility adapters) eventually call this.
pub(crate) async fn execute<F: Fn(AgentTraceEvent)>(
    invoker: &dyn LlmTurnInvoker,
    model: &str,
    tools: &ToolRegistry,
    config: &EngineConfig,
    messages: &mut Vec<ChatMessage>,
    on_event: &F,
    event_sink: Option<Arc<dyn Fn(AgentTraceEvent) + Send + Sync>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    request_headers: Option<Arc<HashMap<String, String>>>,
) -> Result<EngineResult, EngineError> {
    let start_time = std::time::Instant::now();
    let mut turns: u32 = 0;
    let mut total_tool_calls: usize = 0;
    let mut last_usage: Option<llm_client::Usage> = None;
    let mut accumulated_text = String::new();

    loop {
        if cancel_flag
            .as_ref()
            .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Err(EngineError::Cancelled);
        }

        turns += 1;

        // ── Safety gates (checked before each LLM call) ─────────────
        if turns as usize > config.max_turns {
            on_event(AgentTraceEvent::Failed {
                message: format!("Turn limit reached ({})", config.max_turns),
            });
            return Err(EngineError::TurnLimit { turns });
        }
        if let Some(timeout_ms) = config.wall_clock_timeout_ms {
            let elapsed = start_time.elapsed().as_millis() as u64;
            if elapsed >= timeout_ms {
                on_event(AgentTraceEvent::Failed {
                    message: format!("Timeout after {}ms", elapsed),
                });
                return Err(EngineError::Timeout {
                    elapsed_ms: elapsed,
                });
            }
        }
        if let Some(max_tc) = config.max_tool_calls {
            if total_tool_calls >= max_tc {
                on_event(AgentTraceEvent::Failed {
                    message: format!("Tool call limit reached ({})", max_tc),
                });
                return Err(EngineError::ToolCallLimit {
                    count: total_tool_calls,
                });
            }
        }

        // ── Build LLM request ───────────────────────────────────────
        let tool_schemas = build_tool_schemas(tools);

        if let Some(max_tokens) = config.max_context_tokens {
            let evicted = trim_messages_to_budget(messages, max_tokens, &tool_schemas);
            if evicted > 0 {
                on_event(AgentTraceEvent::ContextTrimmed {
                    evicted_count: evicted as u32,
                    remaining_count: messages.len() as u32,
                });
            }
        }

        let mut parallel_ext = serde_json::Map::new();
        parallel_ext.insert(
            "parallel_tool_calls".to_string(),
            serde_json::Value::Bool(false),
        );

        let mut request = LlmRequest {
            model: model.to_string(),
            messages: messages.clone(),
            tools: Some(tool_schemas),
            tool_choice: Some(ToolChoice::Auto),
            extensions: Some(parallel_ext),
            ..Default::default()
        };

        if let Some(ref defaults) = config.request_defaults {
            apply_request_defaults(&mut request, defaults);
        }

        // ── Emit TurnStarted ────────────────────────────────────────
        on_event(AgentTraceEvent::TurnStarted {
            turn: turns,
            response_id: None,
            model: None,
        });

        // ── Invoke LLM turn ─────────────────────────────────────────
        let turn_result = match invoker.invoke_turn(request).await {
            Ok(r) => r,
            Err(e) => {
                on_event(AgentTraceEvent::Failed {
                    message: e.to_string(),
                });
                return Err(e);
            }
        };

        if let Some(u) = &turn_result.usage {
            last_usage = Some(u.clone());
        }

        // ── Execute pending tool calls ──────────────────────────────
        if !turn_result.tool_calls.is_empty() {
            let tool_call_requests: Vec<llm_client::ToolCallRequest> = turn_result
                .tool_calls
                .iter()
                .map(|tc| llm_client::ToolCallRequest {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: serde_json::from_str(&tc.arguments_raw)
                        .unwrap_or_else(|_| serde_json::json!({"_raw": tc.arguments_raw})),
                })
                .collect();
            let assistant_msg = ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(tool_call_requests),
                ..Default::default()
            };
            messages.push(assistant_msg);

            for tc in &turn_result.tool_calls {
                let arguments: serde_json::Value = serde_json::from_str(&tc.arguments_raw)
                    .unwrap_or_else(|_| serde_json::json!({"_raw": tc.arguments_raw}));

                let tool_start = std::time::Instant::now();
                let tool_result = tools
                    .execute_with_context(
                        &tc.name,
                        arguments,
                        event_sink.as_ref().map(|sink| {
                            let cf = cancel_flag
                                .as_ref()
                                .cloned()
                                .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                            match &request_headers {
                                Some(hdrs) => {
                                    ToolContext::with_headers(sink.clone(), cf, hdrs.clone())
                                }
                                None => ToolContext::new(sink.clone(), cf),
                            }
                        }),
                    )
                    .await;
                let duration_ms = tool_start.elapsed().as_millis() as u64;

                match tool_result {
                    Ok(out) => {
                        // Stop signal: sentinel tool (e.g. checkpoint_task) requested loop exit
                        if out.output.get("__engine_stop").and_then(|v| v.as_bool()) == Some(true) {
                            on_event(AgentTraceEvent::ToolCallCompleted {
                                index: tc.index,
                                id: tc.id.clone(),
                                name: tc.name.clone(),
                                result: out.output.clone(),
                                duration_ms,
                                success: true,
                            });
                            return Ok(EngineResult {
                                text: if accumulated_text.is_empty() {
                                    None
                                } else {
                                    Some(accumulated_text)
                                },
                                usage: last_usage,
                                turns_used: turns,
                                tool_calls_made: total_tool_calls + 1,
                                stop_signal: Some(out.output),
                            });
                        }

                        let injected_parts = multimodal_parts_from_tool_output(&out.output);
                        let out_for_tool_result = strip_llm_content_markers(out.output.clone());
                        let out_str = serde_json::to_string(&out_for_tool_result)
                            .unwrap_or_else(|_| out.output.to_string());

                        on_event(AgentTraceEvent::ToolCallCompleted {
                            index: tc.index,
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            result: out_for_tool_result,
                            duration_ms,
                            success: true,
                        });

                        let tool_result_msg = ChatMessage {
                            role: "tool".to_string(),
                            content: Some(out_str),
                            tool_call_id: Some(tc.id.clone()),
                            ..Default::default()
                        };
                        messages.push(tool_result_msg);

                        if let Some((content, content_parts)) = injected_parts {
                            messages.push(ChatMessage {
                                role: "user".to_string(),
                                content,
                                content_parts: Some(content_parts),
                                ..Default::default()
                            });
                        }

                        total_tool_calls += 1;
                    }
                    Err(err) => {
                        let parsed_error = serde_json::from_str::<serde_json::Value>(&err)
                            .unwrap_or_else(|_| serde_json::json!({ "message": err }));
                        on_event(AgentTraceEvent::ToolCallCompleted {
                            index: tc.index,
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            result: serde_json::json!({"error": parsed_error}),
                            duration_ms,
                            success: false,
                        });

                        let error_result_msg = ChatMessage {
                            role: "tool".to_string(),
                            content: Some(serde_json::json!({"error": parsed_error}).to_string()),
                            tool_call_id: Some(tc.id.clone()),
                            ..Default::default()
                        };
                        messages.push(error_result_msg);

                        total_tool_calls += 1;
                    }
                }
            }
            on_event(AgentTraceEvent::TurnCompleted {
                turn: turns,
                finish_reason: turn_result.finish_reason.clone(),
            });
            continue;
        }

        // ── No tool calls → text response, we're done ───────────────
        if !turn_result.content.is_empty() {
            accumulated_text.push_str(&turn_result.content);
        }

        on_event(AgentTraceEvent::TurnCompleted {
            turn: turns,
            finish_reason: turn_result.finish_reason.clone(),
        });

        on_event(AgentTraceEvent::Completed {
            text: if accumulated_text.is_empty() {
                None
            } else {
                Some(accumulated_text.clone())
            },
            usage: last_usage.clone(),
        });

        return Ok(EngineResult {
            text: if accumulated_text.is_empty() {
                None
            } else {
                Some(accumulated_text)
            },
            usage: last_usage,
            turns_used: turns,
            tool_calls_made: total_tool_calls,
            stop_signal: None,
        });
    }
}

// =========================================================================
// Internal helpers
// =========================================================================

fn build_tool_schemas(tools: &ToolRegistry) -> Vec<ToolSchema> {
    tools
        .list()
        .iter()
        .map(|t| ToolSchema {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.parameters.clone(),
            strict: if t.strict { Some(true) } else { None },
        })
        .collect()
}

// ── Context-window management ───────────────────────────────────────────

fn trim_messages_to_budget(
    messages: &mut Vec<ChatMessage>,
    max_tokens: u32,
    tool_schemas: &[ToolSchema],
) -> usize {
    let tools_tokens = estimate_tools_tokens_schemas(tool_schemas);
    let available = max_tokens.saturating_sub(tools_tokens);

    let total = estimate_messages_tokens_chat(messages);
    if total <= available {
        return 0;
    }

    let has_system = messages.first().is_some_and(|m| m.role == "system");

    let system_tokens = if has_system {
        chat_message_as_value_tokens(&messages[0])
    } else {
        0
    };

    let history_budget = available.saturating_sub(system_tokens).saturating_sub(3);
    let start_idx = if has_system { 1 } else { 0 };

    let mut keep_from = messages.len();
    let mut used: u32 = 0;
    for i in (start_idx..messages.len()).rev() {
        let msg_tokens = chat_message_as_value_tokens(&messages[i]);
        if used + msg_tokens > history_budget {
            break;
        }
        used += msg_tokens;
        keep_from = i;
    }

    if keep_from <= start_idx {
        return 0;
    }

    let evicted = keep_from - start_idx;
    messages.drain(start_idx..keep_from);

    debug!(
        "context_trim: evicted {} messages, {} tokens remain (budget={})",
        evicted, used, available
    );

    evicted
}

fn chat_message_as_value_tokens(msg: &ChatMessage) -> u32 {
    serde_json::to_value(msg)
        .ok()
        .as_ref()
        .map(estimate_msg_tokens)
        .unwrap_or(4)
}

fn estimate_msg_tokens(msg: &serde_json::Value) -> u32 {
    let overhead: u32 = 4;
    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let content_tokens = (content.len() as f64 / 4.0).ceil() as u32;
    let mut tokens = overhead + content_tokens;
    if let Some(tcs) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in tcs {
            tokens += 3;
            if let Some(f) = tc.get("function") {
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = f.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                tokens += (name.len() as f64 / 4.0).ceil() as u32;
                tokens += (args.len() as f64 / 4.0).ceil() as u32;
            }
        }
    }
    tokens
}

fn estimate_messages_tokens_chat(messages: &[ChatMessage]) -> u32 {
    3 + messages
        .iter()
        .map(chat_message_as_value_tokens)
        .sum::<u32>()
}

fn estimate_tools_tokens_schemas(tools: &[ToolSchema]) -> u32 {
    if tools.is_empty() {
        return 0;
    }
    let mut tokens: u32 = 10;
    for tool in tools {
        let s = serde_json::to_string(tool).unwrap_or_default();
        tokens += (s.len() as f64 / 4.0).ceil() as u32;
    }
    tokens
}

fn multimodal_parts_from_tool_output(
    output: &serde_json::Value,
) -> Option<(Option<String>, Vec<ChatContentPart>)> {
    let object = output.as_object()?;
    let parts_value = object.get("__llm_content_parts")?;
    let parts = serde_json::from_value::<Vec<ChatContentPart>>(parts_value.clone()).ok()?;
    if parts.is_empty() {
        return None;
    }
    let content = object
        .get("__llm_content_text")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    Some((content, parts))
}

fn strip_llm_content_markers(mut output: serde_json::Value) -> serde_json::Value {
    if let Some(object) = output.as_object_mut() {
        object.remove("__llm_content_parts");
        object.remove("__llm_content_text");
    }
    output
}

// ── Request defaults ────────────────────────────────────────────────────

fn merge_ext_maps(
    a: Option<serde_json::Map<String, serde_json::Value>>,
    b: Option<serde_json::Map<String, serde_json::Value>>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    match (a, b) {
        (None, None) => None,
        (Some(m), None) | (None, Some(m)) => Some(m),
        (Some(mut m1), Some(m2)) => {
            for (k, v) in m2 {
                m1.insert(k, v);
            }
            Some(m1)
        }
    }
}

fn apply_request_defaults(request: &mut LlmRequest, defaults: &LlmRequestDefaults) {
    if let Some(ref model_cfg) = defaults.model_config {
        let prep_input = LlmRequest {
            model: request.model.clone(),
            messages: vec![],
            tools: None,
            tool_choice: None,
            temperature: defaults.temperature.or(request.temperature),
            max_tokens: defaults.max_tokens.or(request.max_tokens),
            extensions: merge_ext_maps(request.extensions.clone(), defaults.extensions.clone()),
        };

        let mutators: Vec<Box<dyn llm_client::prepare::RequestMutator>> = vec![
            Box::new(llm_client::prepare::Gpt5Mutator),
            Box::new(llm_client::prepare::QwenVllmExtras),
        ];
        let validators: Vec<Box<dyn llm_client::prepare::RequestValidator>> =
            vec![Box::new(llm_client::prepare::ProfileCapabilityValidator)];

        match llm_client::prepare::prepare_request(
            model_cfg,
            prep_input,
            &mutators,
            &validators,
            llm_client::prepare::Policy::Permissive,
        ) {
            Ok(prepared) => {
                if prepared.temperature.is_some() {
                    request.temperature = prepared.temperature;
                }
                if prepared.max_tokens.is_some() {
                    request.max_tokens = prepared.max_tokens;
                }
                if let Some(ext) = prepared.extensions {
                    let mut m = request.extensions.clone().unwrap_or_default();
                    for (k, v) in ext {
                        m.insert(k, v);
                    }
                    request.extensions = Some(m);
                }
            }
            Err(e) => {
                log::warn!("prepare_request failed, applying raw defaults: {}", e);
                merge_raw_defaults(request, defaults);
            }
        }
    } else {
        merge_raw_defaults(request, defaults);
    }
}

fn merge_raw_defaults(req: &mut LlmRequest, defaults: &LlmRequestDefaults) {
    if let Some(t) = defaults.temperature {
        req.temperature = Some(t);
    }
    if let Some(mt) = defaults.max_tokens {
        req.max_tokens = Some(mt);
    }
    if let Some(ref ext) = defaults.extensions {
        let mut m = req.extensions.take().unwrap_or_default();
        for (k, v) in ext {
            m.insert(k.clone(), v.clone());
        }
        req.extensions = Some(m);
    }
}
