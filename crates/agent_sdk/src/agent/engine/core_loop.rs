//! Core tool-calling execution loop — the single source of truth.
//!
//! This module contains the protocol-agnostic inner loop that drives
//! LLM → tool calls → LLM iterations. It operates on raw JSON messages
//! and emits [`AgentTraceEvent`]s through a callback.
//!
//! No A2A types, no `MessageContext`, no `TaskContext`, no `checkpoint_task`.
//! Works on both WASM (via `RequestResponseTurnInvoker`) and native
//! (via `StreamingTurnInvoker`).

use crate::agent::llm_invoker::LlmRequestDefaults;
use crate::agent::tool_context::ToolContext;
use crate::agent::tools::ToolRegistry;
use crate::agent::trace::AgentTraceEvent;
use log::debug;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

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
    messages: &mut Vec<serde_json::Value>,
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

        // ── Build LLM payload ───────────────────────────────────────
        let tools_json = build_tools_json(tools);

        if let Some(max_tokens) = config.max_context_tokens {
            let evicted = trim_messages_to_budget(messages, max_tokens, &tools_json);
            if evicted > 0 {
                on_event(AgentTraceEvent::ContextTrimmed {
                    evicted_count: evicted as u32,
                    remaining_count: messages.len() as u32,
                });
            }
        }

        let mut payload = serde_json::json!({
            "model": model,
            "messages": &*messages,
            "tools": tools_json,
            "tool_choice": "auto",
            "parallel_tool_calls": false
        });
        if let Some(ref defaults) = config.request_defaults {
            apply_request_defaults(&mut payload, defaults);
        }

        // ── Emit TurnStarted ────────────────────────────────────────
        on_event(AgentTraceEvent::TurnStarted {
            turn: turns,
            response_id: None,
            model: None,
        });

        // ── Invoke LLM turn ─────────────────────────────────────────
        let turn_result = match invoker.invoke_turn(payload).await {
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
            let tc_history: Vec<serde_json::Value> = turn_result
                .tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments_raw
                        }
                    })
                })
                .collect();
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": serde_json::Value::Null,
                "tool_calls": tc_history
            }));

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

                        let out_str = serde_json::to_string(&out.output)
                            .unwrap_or_else(|_| out.output.to_string());

                        on_event(AgentTraceEvent::ToolCallCompleted {
                            index: tc.index,
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            result: out.output.clone(),
                            duration_ms,
                            success: true,
                        });

                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": out_str
                        }));

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

                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tc.id,
                            "content": serde_json::json!({"error": parsed_error}).to_string()
                        }));

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

fn build_tools_json(tools: &ToolRegistry) -> Vec<serde_json::Value> {
    tools
        .list()
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                }
            })
        })
        .collect()
}

// ── Context-window management ───────────────────────────────────────────

fn trim_messages_to_budget(
    messages: &mut Vec<serde_json::Value>,
    max_tokens: u32,
    tools_json: &[serde_json::Value],
) -> usize {
    let tools_tokens = estimate_tools_tokens(tools_json);
    let available = max_tokens.saturating_sub(tools_tokens);

    let total = estimate_messages_tokens(messages);
    if total <= available {
        return 0;
    }

    let has_system = messages
        .first()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
        == Some("system");

    let system_tokens = if has_system {
        estimate_msg_tokens(&messages[0])
    } else {
        0
    };

    let history_budget = available.saturating_sub(system_tokens).saturating_sub(3);
    let start_idx = if has_system { 1 } else { 0 };

    let mut keep_from = messages.len();
    let mut used: u32 = 0;
    for i in (start_idx..messages.len()).rev() {
        let msg_tokens = estimate_msg_tokens(&messages[i]);
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

fn estimate_messages_tokens(messages: &[serde_json::Value]) -> u32 {
    3 + messages.iter().map(estimate_msg_tokens).sum::<u32>()
}

fn estimate_tools_tokens(tools: &[serde_json::Value]) -> u32 {
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

// ── Request defaults ────────────────────────────────────────────────────

fn apply_request_defaults(payload: &mut serde_json::Value, defaults: &LlmRequestDefaults) {
    let obj = match payload.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    if let Some(ref model_cfg) = defaults.model_config {
        let req = llm_client::LlmRequest {
            model: obj
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            temperature: defaults.temperature,
            max_tokens: defaults.max_tokens,
            extensions: defaults.extensions.clone(),
            ..Default::default()
        };

        let mutators: Vec<Box<dyn llm_client::prepare::RequestMutator>> = vec![
            Box::new(llm_client::prepare::Gpt5Mutator),
            Box::new(llm_client::prepare::QwenVllmExtras),
        ];
        let validators: Vec<Box<dyn llm_client::prepare::RequestValidator>> =
            vec![Box::new(llm_client::prepare::ProfileCapabilityValidator)];

        match llm_client::prepare::prepare_request(
            model_cfg,
            req,
            &mutators,
            &validators,
            llm_client::prepare::Policy::Permissive,
        ) {
            Ok(prepared) => {
                if let Some(t) = prepared.temperature {
                    obj.insert("temperature".to_string(), serde_json::json!(t));
                }
                if let Some(mt) = prepared.max_tokens {
                    obj.insert("max_tokens".to_string(), serde_json::json!(mt));
                }
                if let Some(ext) = prepared.extensions {
                    for (k, v) in ext {
                        obj.insert(k, v);
                    }
                }
            }
            Err(e) => {
                log::warn!("prepare_request failed, applying raw defaults: {}", e);
                merge_raw_defaults(obj, defaults);
            }
        }
    } else {
        merge_raw_defaults(obj, defaults);
    }
}

fn merge_raw_defaults(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    defaults: &LlmRequestDefaults,
) {
    if let Some(t) = defaults.temperature {
        obj.insert("temperature".to_string(), serde_json::json!(t));
    }
    if let Some(mt) = defaults.max_tokens {
        obj.insert("max_tokens".to_string(), serde_json::json!(mt));
    }
    if let Some(ref ext) = defaults.extensions {
        for (k, v) in ext {
            obj.insert(k.clone(), v.clone());
        }
    }
}
