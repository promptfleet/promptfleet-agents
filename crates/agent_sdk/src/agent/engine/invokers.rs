//! LLM turn invoker implementations.
//!
//! - [`RequestResponseTurnInvoker`]: wraps `LlmInvoker` (WASM + native).
//!   Sends a JSON payload, parses the response into [`TurnResult`].
//! - [`StreamingTurnInvoker`]: wraps `LlmStreamInvoker` (native-only).
//!   Consumes the SSE stream, accumulates content + tool calls into
//!   [`TurnResult`], and pushes `ContentDelta`/`ReasoningDelta` events
//!   to a captured sink in real-time.

use super::types::{EngineError, LlmTurnInvoker, ToolCallInfo, TurnFuture, TurnResult};
use crate::agent::llm_invoker::LlmInvoker;
use std::sync::Arc;
#[cfg(feature = "agent-observability")]
use std::sync::OnceLock;
use std::time::Instant;

#[cfg(not(target_arch = "wasm32"))]
use crate::agent::llm_invoker::LlmStreamInvoker;
#[cfg(not(target_arch = "wasm32"))]
use crate::agent::trace::AgentTraceEvent;

#[cfg(feature = "agent-observability")]
use observability::{attr, metric, span, value, ObsHandle, SpanGuard, SpanStatus};

// ===========================================================================
// Request-response invoker (WASM + native)
// ===========================================================================

/// Wraps an [`LlmInvoker`] (request-response) and parses the JSON response
/// into a [`TurnResult`].
pub struct RequestResponseTurnInvoker {
    inner: Arc<dyn LlmInvoker>,
    #[cfg(feature = "agent-observability")]
    obs: Option<observability::Obs>,
}

impl RequestResponseTurnInvoker {
    pub fn new(inner: Arc<dyn LlmInvoker>) -> Self {
        Self {
            inner,
            #[cfg(feature = "agent-observability")]
            obs: obs_from_env_cached(),
        }
    }

    #[cfg(feature = "agent-observability")]
    pub fn new_with_observability(inner: Arc<dyn LlmInvoker>, obs: observability::Obs) -> Self {
        Self {
            inner,
            obs: Some(obs),
        }
    }
}

impl LlmTurnInvoker for RequestResponseTurnInvoker {
    fn invoke_turn(&self, payload: serde_json::Value) -> TurnFuture {
        let inner = self.inner.clone();
        #[cfg(feature = "agent-observability")]
        let obs = self.obs.clone();
        Box::pin(async move {
            let model = extract_model_name(&payload);
            let provider = infer_provider(&model);
            let operation = infer_operation(&payload);
            let started = Instant::now();

            #[cfg(feature = "agent-observability")]
            let span_guard = obs.as_ref().map(|o| {
                o.span(
                    span::LLM_REQUEST,
                    &[
                        (attr::COMPONENT, "llm_client"),
                        (attr::LLM_PROVIDER, provider.as_str()),
                        (attr::LLM_MODEL, model.as_str()),
                        (attr::LLM_OPERATION, operation.as_str()),
                    ],
                )
            });

            let result = inner
                .request(payload)
                .await
                .map_err(|e| EngineError::LlmFailed(format!("LLM request failed: {}", e)));

            #[cfg(feature = "agent-observability")]
            {
                let status = if result.is_ok() {
                    value::STATUS_OK
                } else {
                    value::STATUS_ERROR
                };
                let error_type = result.as_ref().err().map(classify_llm_error);
                emit_llm_request_outcome(
                    obs.as_ref(),
                    span_guard.as_ref(),
                    provider.as_str(),
                    model.as_str(),
                    operation.as_str(),
                    started,
                    status,
                    error_type.as_deref(),
                );
            }

            let raw = result?;

            let maybe_msg = raw
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|ch| ch.get("message"));

            let content = maybe_msg
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let tool_calls_raw = maybe_msg
                .and_then(|m| m.get("tool_calls"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let tool_calls: Vec<ToolCallInfo> = tool_calls_raw
                .iter()
                .enumerate()
                .map(|(i, tc)| ToolCallInfo {
                    index: tc.get("index").and_then(|v| v.as_u64()).unwrap_or(i as u64) as u32,
                    id: tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    arguments_raw: tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect();

            let finish_reason = raw
                .get("choices")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|ch| ch.get("finish_reason"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let usage = raw.get("usage").and_then(|u| {
                Some(llm_client::Usage {
                    prompt_tokens: u
                        .get("prompt_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    completion_tokens: u
                        .get("completion_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    total_tokens: u
                        .get("total_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                })
            });

            #[cfg(feature = "agent-observability")]
            if let (Some(o), Some(u)) = (&obs, &usage) {
                if let Some(in_tokens) = u.prompt_tokens {
                    o.metric(
                        metric::LLM_TOKENS_TOTAL,
                        in_tokens as f64,
                        &[
                            ("provider", provider.as_str()),
                            ("model", model.as_str()),
                            ("direction", value::DIRECTION_INPUT),
                        ],
                    );
                }
                if let Some(out_tokens) = u.completion_tokens {
                    o.metric(
                        metric::LLM_TOKENS_TOTAL,
                        out_tokens as f64,
                        &[
                            ("provider", provider.as_str()),
                            ("model", model.as_str()),
                            ("direction", value::DIRECTION_OUTPUT),
                        ],
                    );
                }
            }

            Ok(TurnResult {
                content,
                tool_calls,
                finish_reason,
                usage,
            })
        })
    }
}

// ===========================================================================
// Streaming invoker (native-only)
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
struct ToolCallAccumulator {
    index: u32,
    id: String,
    name: String,
    arguments_buf: String,
}

/// Wraps an [`LlmStreamInvoker`] and accumulates the SSE stream into a
/// [`TurnResult`], while pushing `ContentDelta`/`ReasoningDelta` events
/// to the captured event sink in real-time.
#[cfg(not(target_arch = "wasm32"))]
pub struct StreamingTurnInvoker {
    inner: Arc<dyn LlmStreamInvoker>,
    event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
    #[cfg(feature = "agent-observability")]
    obs: Option<observability::Obs>,
}

#[cfg(not(target_arch = "wasm32"))]
impl StreamingTurnInvoker {
    pub fn new(
        inner: Arc<dyn LlmStreamInvoker>,
        event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
    ) -> Self {
        Self {
            inner,
            event_sink,
            #[cfg(feature = "agent-observability")]
            obs: obs_from_env_cached(),
        }
    }

    #[cfg(feature = "agent-observability")]
    pub fn new_with_observability(
        inner: Arc<dyn LlmStreamInvoker>,
        event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
        obs: observability::Obs,
    ) -> Self {
        Self {
            inner,
            event_sink,
            obs: Some(obs),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl LlmTurnInvoker for StreamingTurnInvoker {
    fn invoke_turn(&self, payload: serde_json::Value) -> TurnFuture {
        use futures::StreamExt;

        let inner = self.inner.clone();
        let sink = self.event_sink.clone();
        #[cfg(feature = "agent-observability")]
        let obs = self.obs.clone();

        Box::pin(async move {
            let model = extract_model_name(&payload);
            let provider = infer_provider(&model);
            let operation = infer_operation(&payload);
            let started = Instant::now();

            #[cfg(feature = "agent-observability")]
            let span_guard = obs.as_ref().map(|o| {
                o.span(
                    span::LLM_REQUEST,
                    &[
                        (attr::COMPONENT, "llm_client"),
                        (attr::LLM_PROVIDER, provider.as_str()),
                        (attr::LLM_MODEL, model.as_str()),
                        (attr::LLM_OPERATION, operation.as_str()),
                    ],
                )
            });

            let mut event_stream = match inner.request_stream(payload).await {
                Ok(stream) => stream,
                Err(e) => {
                    let err =
                        EngineError::LlmFailed(format!("LLM streaming request failed: {}", e));
                    #[cfg(feature = "agent-observability")]
                    {
                        let error_type = classify_llm_error(&err);
                        emit_llm_request_outcome(
                            obs.as_ref(),
                            span_guard.as_ref(),
                            provider.as_str(),
                            model.as_str(),
                            operation.as_str(),
                            started,
                            value::STATUS_ERROR,
                            Some(error_type.as_str()),
                        );
                    }
                    return Err(err);
                }
            };

            let mut content = String::new();
            let mut pending: Vec<ToolCallAccumulator> = Vec::new();
            let mut finish_reason: Option<String> = None;
            let mut usage: Option<llm_client::Usage> = None;
            let mut reasoning_active = false;
            let mut reasoning_seq: u64 = 0;
            let mut reasoning_message_id = String::new();

            while let Some(event_result) = event_stream.next().await {
                match event_result {
                    Ok(llm_client::StreamEvent::StreamStart { .. }) => {
                        // Metadata-only; response_id/model are not forwarded
                        // because the core loop doesn't track them.
                    }

                    Ok(llm_client::StreamEvent::ContentDelta { delta }) => {
                        if reasoning_active {
                            sink(AgentTraceEvent::ReasoningCompleted {
                                message_id: reasoning_message_id.clone(),
                            });
                            reasoning_active = false;
                        }
                        content.push_str(&delta);
                        sink(AgentTraceEvent::ContentDelta { delta });
                    }

                    Ok(llm_client::StreamEvent::ReasoningDelta { delta }) => {
                        if !reasoning_active {
                            reasoning_seq += 1;
                            reasoning_message_id = format!("reasoning-{reasoning_seq}");
                            sink(AgentTraceEvent::ReasoningStarted {
                                message_id: reasoning_message_id.clone(),
                            });
                            reasoning_active = true;
                        }
                        sink(AgentTraceEvent::ReasoningDelta { delta });
                    }

                    Ok(llm_client::StreamEvent::ToolCallStart { index, id, name }) => {
                        if reasoning_active {
                            sink(AgentTraceEvent::ReasoningCompleted {
                                message_id: reasoning_message_id.clone(),
                            });
                            reasoning_active = false;
                        }
                        sink(AgentTraceEvent::ToolCallStarted {
                            index,
                            id: id.clone(),
                            name: name.clone(),
                            arguments: serde_json::Value::Null,
                        });
                        pending.push(ToolCallAccumulator {
                            index,
                            id,
                            name,
                            arguments_buf: String::new(),
                        });
                    }

                    Ok(llm_client::StreamEvent::ToolCallDelta {
                        index,
                        arguments_delta,
                    }) => {
                        if reasoning_active {
                            sink(AgentTraceEvent::ReasoningCompleted {
                                message_id: reasoning_message_id.clone(),
                            });
                            reasoning_active = false;
                        }
                        if let Some(acc) = pending.iter_mut().find(|a| a.index == index) {
                            sink(AgentTraceEvent::ToolCallArgsDelta {
                                index,
                                id: acc.id.clone(),
                                delta: arguments_delta.clone(),
                            });
                            acc.arguments_buf.push_str(&arguments_delta);
                        }
                    }

                    Ok(llm_client::StreamEvent::Done {
                        finish_reason: fr,
                        usage: u,
                    }) => {
                        if reasoning_active {
                            sink(AgentTraceEvent::ReasoningCompleted {
                                message_id: reasoning_message_id.clone(),
                            });
                            reasoning_active = false;
                        }
                        finish_reason = fr;
                        usage = u;
                    }

                    Ok(llm_client::StreamEvent::Error { message }) => {
                        let err = EngineError::StreamError(message);
                        #[cfg(feature = "agent-observability")]
                        {
                            let error_type = classify_llm_error(&err);
                            emit_llm_request_outcome(
                                obs.as_ref(),
                                span_guard.as_ref(),
                                provider.as_str(),
                                model.as_str(),
                                operation.as_str(),
                                started,
                                value::STATUS_ERROR,
                                Some(error_type.as_str()),
                            );
                        }
                        return Err(err);
                    }

                    Err(e) => {
                        let err = EngineError::StreamError(format!("Stream error: {}", e));
                        #[cfg(feature = "agent-observability")]
                        {
                            let error_type = classify_llm_error(&err);
                            emit_llm_request_outcome(
                                obs.as_ref(),
                                span_guard.as_ref(),
                                provider.as_str(),
                                model.as_str(),
                                operation.as_str(),
                                started,
                                value::STATUS_ERROR,
                                Some(error_type.as_str()),
                            );
                        }
                        return Err(err);
                    }
                }
            }

            let tool_calls: Vec<ToolCallInfo> = pending
                .into_iter()
                .map(|tc| {
                    let arguments = serde_json::from_str::<serde_json::Value>(&tc.arguments_buf)
                        .unwrap_or_else(|_| serde_json::json!({ "_raw": tc.arguments_buf }));
                    sink(AgentTraceEvent::ToolCallArgsCompleted {
                        index: tc.index,
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        arguments,
                    });
                    ToolCallInfo {
                        index: tc.index,
                        id: tc.id,
                        name: tc.name,
                        arguments_raw: tc.arguments_buf,
                    }
                })
                .collect();

            #[cfg(feature = "agent-observability")]
            if let Some(o) = &obs {
                let status = if finish_reason.as_deref() == Some("error") {
                    value::STATUS_ERROR
                } else {
                    value::STATUS_OK
                };
                let error_type = if status == value::STATUS_ERROR {
                    Some("stream_error")
                } else {
                    None
                };

                emit_llm_request_outcome(
                    obs.as_ref(),
                    span_guard.as_ref(),
                    provider.as_str(),
                    model.as_str(),
                    operation.as_str(),
                    started,
                    status,
                    error_type,
                );

                if let Some(u) = &usage {
                    if let Some(in_tokens) = u.prompt_tokens {
                        o.metric(
                            metric::LLM_TOKENS_TOTAL,
                            in_tokens as f64,
                            &[
                                ("provider", provider.as_str()),
                                ("model", model.as_str()),
                                ("direction", value::DIRECTION_INPUT),
                            ],
                        );
                    }
                    if let Some(out_tokens) = u.completion_tokens {
                        o.metric(
                            metric::LLM_TOKENS_TOTAL,
                            out_tokens as f64,
                            &[
                                ("provider", provider.as_str()),
                                ("model", model.as_str()),
                                ("direction", value::DIRECTION_OUTPUT),
                            ],
                        );
                    }
                }
            }

            Ok(TurnResult {
                content,
                tool_calls,
                finish_reason,
                usage,
            })
        })
    }
}

fn extract_model_name(payload: &serde_json::Value) -> String {
    payload
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn infer_operation(payload: &serde_json::Value) -> String {
    if payload.get("tools").and_then(|v| v.as_array()).is_some() {
        "chat_completions_tools".to_string()
    } else {
        "chat_completions".to_string()
    }
}

fn infer_provider(model: &str) -> String {
    let model_lc = model.to_ascii_lowercase();
    if let Some((prefix, _)) = model_lc.split_once('/') {
        return prefix.to_string();
    }
    if model_lc.starts_with("gpt") {
        return "openai".to_string();
    }
    if model_lc.starts_with("claude") {
        return "anthropic".to_string();
    }
    if model_lc.starts_with("gemini") {
        return "google".to_string();
    }
    if model_lc.starts_with("qwen") {
        return "qwen".to_string();
    }
    if model_lc.starts_with("deepseek") {
        return "deepseek".to_string();
    }
    "unknown".to_string()
}

#[cfg(feature = "agent-observability")]
fn classify_llm_error(err: &EngineError) -> String {
    if matches!(err, EngineError::StreamError(_)) {
        return "stream_error".to_string();
    }

    let s = err.to_string().to_ascii_lowercase();
    if s.contains("timeout") {
        "timeout".to_string()
    } else if s.contains("rate") && s.contains("limit") {
        "rate_limit".to_string()
    } else if s.contains("transport") || s.contains("network") || s.contains("http") {
        "transport".to_string()
    } else if s.contains("invalid") || s.contains("serialization") || s.contains("parse") {
        "invalid_request".to_string()
    } else {
        "provider_error".to_string()
    }
}

#[cfg(feature = "agent-observability")]
fn obs_from_env_cached() -> Option<observability::Obs> {
    static OBS: OnceLock<Option<observability::Obs>> = OnceLock::new();
    OBS.get_or_init(|| observability::Obs::init_from_env().ok())
        .clone()
}

#[cfg(feature = "agent-observability")]
fn emit_llm_request_outcome(
    obs: Option<&observability::Obs>,
    span_guard: Option<&SpanGuard>,
    provider: &str,
    model: &str,
    operation: &str,
    started: Instant,
    status: &str,
    error_type: Option<&str>,
) {
    if let Some(o) = obs {
        o.metric(
            metric::LLM_REQUESTS_TOTAL,
            1.0,
            &[
                ("provider", provider),
                ("model", model),
                ("operation", operation),
                (attr::STATUS, status),
            ],
        );
        o.metric(
            metric::LLM_LATENCY_MS,
            started.elapsed().as_secs_f64() * 1000.0,
            &[
                ("provider", provider),
                ("model", model),
                ("operation", operation),
                (attr::STATUS, status),
            ],
        );
    }

    if let Some(g) = span_guard {
        g.add_attribute(attr::STATUS, status);
        g.add_attribute(
            attr::PF_OUTCOME,
            if status == value::STATUS_OK {
                value::OUTCOME_OK
            } else {
                value::OUTCOME_ERROR
            },
        );
        if let Some(kind) = error_type {
            g.add_attribute(attr::ERROR_TYPE, kind);
        }
        g.set_status(if status == value::STATUS_OK {
            SpanStatus::Ok
        } else {
            SpanStatus::Error
        });
    }
}
