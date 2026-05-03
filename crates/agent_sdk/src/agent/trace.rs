//! Agent trace event types — the SDK-internal execution trace.
//!
//! [`AgentTraceEvent`] is the canonical output of the agent's execution core.
//! It is protocol-agnostic: adapters map it to whatever external protocol is
//! needed (A2A SSE, Studio RunEvents, MCP responses, etc.).
//!
//! # Event sequence
//!
//! **Text response:**
//! `TurnStarted → ContentDelta* → TurnCompleted → Completed`
//!
//! **Tool calls:**
//! `TurnStarted → ToolCallStarted → ToolCallCompleted → TurnStarted → ContentDelta* → TurnCompleted → Completed`
//!
//! **Reasoning models:**
//! `TurnStarted → ReasoningDelta* → ContentDelta* → TurnCompleted → Completed`
//!
//! **Progress updates (any phase):**
//! `... → ProgressUpdate → ...`
//!
//! **Error:**
//! `TurnStarted → ... → Failed`

use serde::{Deserialize, Serialize};

use crate::interaction::{InteractionRequest, InteractionResponse};

/// A single trace event from the agent's execution core.
///
/// These events are emitted in real-time as the agent processes a message.
/// They are SDK-internal and do not cross network boundaries. External
/// protocol adapters (A2A, Studio, MCP) consume these and map them to
/// their respective wire formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentTraceEvent {
    /// A new LLM turn started within the tools loop.
    TurnStarted {
        /// 1-based turn number
        turn: u32,
        /// Provider-assigned response ID (if available from stream start)
        response_id: Option<String>,
        /// Model name
        model: Option<String>,
    },

    /// A text content delta from the assistant (streamed per-token).
    ContentDelta {
        /// The text fragment
        delta: String,
    },

    /// A reasoning/thinking phase started.
    ReasoningStarted {
        /// Reasoning stream message id.
        message_id: String,
    },

    /// A reasoning/thinking delta from reasoning models.
    ///
    /// Only emitted when the upstream provider natively streams reasoning
    /// tokens (Qwen3 `reasoning_content`, DeepSeek R1, etc.).
    /// **Never fabricated.**
    ReasoningDelta {
        /// The reasoning text fragment
        delta: String,
    },

    /// A reasoning/thinking phase completed.
    ReasoningCompleted {
        /// Reasoning stream message id.
        message_id: String,
    },

    /// A tool call was announced by the LLM.
    ///
    /// In the streaming path this is emitted immediately when the LLM
    /// signals a tool call (before argument deltas arrive). The
    /// `arguments` field is `Value::Null` at this point; the full
    /// arguments are available in [`AgentTraceEvent::ToolCallArgsCompleted`].
    ToolCallStarted {
        /// Tool call index within this turn (for parallel calls)
        index: u32,
        /// Unique ID for this tool call (e.g. `"call_xxx"`)
        id: String,
        /// Function name
        name: String,
        /// Fully assembled arguments (parsed JSON)
        arguments: serde_json::Value,
    },

    /// A streamed delta of tool call arguments.
    ToolCallArgsDelta {
        /// Tool call index within this turn (for parallel calls)
        index: u32,
        /// Unique ID for this tool call
        id: String,
        /// Raw argument JSON fragment
        delta: String,
    },

    /// Tool call arguments finished streaming and were assembled.
    ToolCallArgsCompleted {
        /// Tool call index within this turn (for parallel calls)
        index: u32,
        /// Unique ID for this tool call
        id: String,
        /// Function name
        name: String,
        /// Fully assembled arguments (parsed JSON)
        arguments: serde_json::Value,
    },

    /// A tool call finished execution.
    ToolCallCompleted {
        /// Tool call index (matches the `index` field on [`AgentTraceEvent::ToolCallStarted`])
        index: u32,
        /// Unique ID (matches the `id` field on [`AgentTraceEvent::ToolCallStarted`])
        id: String,
        /// Function name
        name: String,
        /// Tool output (JSON value)
        result: serde_json::Value,
        /// Execution duration in milliseconds
        duration_ms: u64,
        /// Whether the tool execution was successful
        success: bool,
    },

    /// Governance evaluated a tool call before execution.
    GovernanceDecision {
        /// Function/tool name.
        name: String,
        /// Persisted governance decision id when the control plane created one.
        decision_id: Option<String>,
        /// Effective governance mode: disabled, audit, or enforce.
        mode: String,
        /// Decision value returned by policy evaluation.
        decision: String,
        /// True when audit mode recorded a would-have decision.
        would_have: bool,
        /// Human-readable reason.
        reason: String,
    },

    /// An LLM turn completed.
    TurnCompleted {
        /// 1-based turn number
        turn: u32,
        /// LLM finish reason: `"stop"`, `"tool_calls"`, `"length"`, etc.
        finish_reason: Option<String>,
    },

    /// The agent execution completed successfully.
    Completed {
        /// The full accumulated assistant text (if any)
        text: Option<String>,
        /// Token usage statistics (if available)
        usage: Option<llm_client::Usage>,
    },

    /// Context window was trimmed to fit within the token budget.
    ///
    /// Emitted when `LlmPolicy::max_context_tokens` is set and the
    /// message history exceeds the budget before an LLM call.
    ContextTrimmed {
        /// Number of messages evicted from history.
        evicted_count: u32,
        /// Number of messages remaining after trimming.
        remaining_count: u32,
    },

    /// Progress update for long-running operations.
    ///
    /// Emitted by the engine when a significant step completes or a
    /// meaningful progress milestone is reached (e.g. "Searching the web…",
    /// "Analysing results…"). The frontend maps this to an activity
    /// timeline entry so the user sees real-time status.
    ProgressUpdate {
        /// Human-readable status message (e.g. "Searching for relevant papers…")
        message: String,
        /// Optional progress percentage (0–100). `None` = indeterminate.
        progress_pct: Option<u8>,
        /// Optional metadata for the frontend (e.g. step name, phase)
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// Logical control handoff between the current agent and a sub-agent.
    AgentHandoff {
        /// The current/previous agent identifier.
        from_agent: String,
        /// The target/next agent identifier.
        to_agent: String,
        /// Optional metadata for UI and logging layers.
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },

    /// The agent requested explicit user interaction (question/confirmation).
    InteractionRequested { request: InteractionRequest },

    /// A previously requested interaction was resolved.
    InteractionResolved { response: InteractionResponse },

    /// A previously requested interaction was cancelled.
    InteractionCancelled {
        interaction_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },

    /// A previously requested interaction expired without a response.
    InteractionExpired { interaction_id: String },

    /// The agent execution failed.
    Failed {
        /// Human-readable error description
        message: String,
    },
}

/// A boxed, pinned, `Send` stream of [`AgentTraceEvent`]s.
///
/// This is the canonical return type for the streaming tools loop.
/// Protocol adapters consume this stream and map events to their
/// respective wire formats (A2A, Studio, MCP).
#[cfg(not(target_arch = "wasm32"))]
pub type AgentTraceStream = std::pin::Pin<Box<dyn futures::Stream<Item = AgentTraceEvent> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_event_serializes_as_tagged() {
        let event = AgentTraceEvent::ContentDelta {
            delta: "Hello".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "content_delta");
        assert_eq!(json["delta"], "Hello");
    }

    #[test]
    fn trace_event_tool_call_started_roundtrip() {
        let event = AgentTraceEvent::ToolCallStarted {
            index: 0,
            id: "call_123".into(),
            name: "get_weather".into(),
            arguments: serde_json::json!({"city": "Paris"}),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: AgentTraceEvent = serde_json::from_str(&json_str).unwrap();
        match parsed {
            AgentTraceEvent::ToolCallStarted {
                name, arguments, ..
            } => {
                assert_eq!(name, "get_weather");
                assert_eq!(arguments["city"], "Paris");
            }
            other => panic!("expected ToolCallStarted, got {:?}", other),
        }
    }

    #[test]
    fn trace_event_agent_handoff_roundtrip() {
        let event = AgentTraceEvent::AgentHandoff {
            from_agent: "coordinator".into(),
            to_agent: "planner".into(),
            metadata: Some(serde_json::json!({"mode": "streaming"})),
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: AgentTraceEvent = serde_json::from_str(&json_str).unwrap();
        match parsed {
            AgentTraceEvent::AgentHandoff {
                from_agent,
                to_agent,
                metadata,
            } => {
                assert_eq!(from_agent, "coordinator");
                assert_eq!(to_agent, "planner");
                assert_eq!(metadata.unwrap()["mode"], "streaming");
            }
            other => panic!("expected AgentHandoff, got {:?}", other),
        }
    }

    #[test]
    fn trace_event_agent_handoff_without_metadata() {
        let event = AgentTraceEvent::AgentHandoff {
            from_agent: "a".into(),
            to_agent: "b".into(),
            metadata: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "agent_handoff");
        assert_eq!(json["from_agent"], "a");
        assert!(
            json.get("metadata").is_none(),
            "None metadata should be skipped"
        );
    }

    #[test]
    fn trace_event_completed_with_usage() {
        let event = AgentTraceEvent::Completed {
            text: Some("42".into()),
            usage: Some(llm_client::Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            }),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "completed");
        assert_eq!(json["text"], "42");
        assert_eq!(json["usage"]["total_tokens"], 15);
    }

    #[test]
    fn trace_event_interaction_requested_roundtrip() {
        let event = AgentTraceEvent::InteractionRequested {
            request: InteractionRequest {
                interaction_id: "ix-1".into(),
                kind: crate::interaction::InteractionKind::Confirmation,
                question: "Proceed?".into(),
                options: vec![],
                allow_free_text: true,
                allow_cancel: true,
                default_option_id: None,
                timeout_ms: Some(30_000),
                continuation_id: Some("cont-1".into()),
                source_node: Some("approve_node".into()),
                metadata: None,
            },
        };
        let json_str = serde_json::to_string(&event).unwrap();
        let parsed: AgentTraceEvent = serde_json::from_str(&json_str).unwrap();
        match parsed {
            AgentTraceEvent::InteractionRequested { request } => {
                assert_eq!(request.interaction_id, "ix-1");
                assert_eq!(request.question, "Proceed?");
            }
            other => panic!("expected InteractionRequested, got {:?}", other),
        }
    }
}
