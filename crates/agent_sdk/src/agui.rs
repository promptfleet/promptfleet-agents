//! AG-UI adapter surface for composing user-facing streaming around [`crate::Agent`].
//!
//! ## Platform
//!
//! - **Native only** (`not(target_arch = "wasm32")`): HTTP routes and SSE streaming require Tokio
//!   and Axum. Enable the **`event-stream`** feature (and typically **`llm-engine`** on the agent).
//! - **WASM**: this module is not built for `wasm32`; use A2A hosting from [`crate::host`] or
//!   `a2a_serve!` instead.
//!
//! ## Usage
//!
//! - Configure [`crate::agui::AgUiConfig`] (route path) and build [`crate::agui::AgUiApp`] from an [`crate::Agent`] that has a
//!   **stream-capable** LLM runtime ([`crate::Agent::configure_llm_runtime`] on native).
//! - Combine with [`crate::AgentHostBuilder::with_agui`] to serve AG-UI alongside A2A on one router.

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::collections::HashMap;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::atomic::AtomicBool;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use std::sync::{Arc, Mutex};

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use agent_core::{AgentMessage, ContentPart, ConversationContext, Role};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::extract::State;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::http::HeaderMap;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::response::{IntoResponse, Response};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::routing::post;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use axum::{Json, Router};
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use futures_util::StreamExt;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use serde::Deserialize;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use uuid::Uuid;

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use crate::agent::trace::AgentTraceEvent;
#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
use crate::{Agent, SdkError, SdkResult};

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
pub use crate::streaming::{
    AgUiDriverConfig, AgUiStream, AgUiStreamDriver, AgentIoEvent, IoEventContext, RunStatus,
    RunSummary, StreamEnricher, SummaryHandle, ag_ui_sse_response, ag_ui_sse_response_with_summary,
    agent_io_sse_stream, map_trace_to_agent_io,
};

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone)]
pub struct AgUiConfig {
    pub route_path: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl Default for AgUiConfig {
    fn default() -> Self {
        Self {
            route_path: "/agui/v1/runs".to_string(),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Clone)]
pub struct AgUiApp {
    config: AgUiConfig,
    state: Arc<AgUiState>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl AgUiApp {
    pub fn from_agent(agent: Agent, config: AgUiConfig) -> SdkResult<Self> {
        Self::from_shared_agent(Arc::new(agent), config)
    }

    pub fn from_shared_agent(agent: Arc<Agent>, config: AgUiConfig) -> SdkResult<Self> {
        #[cfg(all(feature = "llm-engine", not(target_arch = "wasm32")))]
        if !agent.can_run_stream() {
            return Err(SdkError::configuration(
                "AG-UI app requires a stream-capable LLM runtime; configure the agent with configure_llm_runtime",
            ));
        }

        Ok(Self {
            config,
            state: Arc::new(AgUiState::new(agent)),
        })
    }

    pub fn router(self) -> Router {
        Router::new()
            .route(&self.config.route_path, post(agui_run))
            .with_state(self.state)
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingInteraction {
    interaction_id: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Default)]
struct ThreadConversationState {
    history: Vec<AgentMessage>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
struct AgUiState {
    agent: Arc<Agent>,
    pending_interactions: Mutex<HashMap<String, PendingInteraction>>,
    thread_conversations: Mutex<HashMap<String, ThreadConversationState>>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl AgUiState {
    fn new(agent: Arc<Agent>) -> Self {
        Self {
            agent,
            pending_interactions: Mutex::new(HashMap::new()),
            thread_conversations: Mutex::new(HashMap::new()),
        }
    }

    fn record_pending_interaction(&self, thread_id: String, interaction_id: String) {
        self.pending_interactions
            .lock()
            .expect("pending interaction lock poisoned")
            .insert(thread_id, PendingInteraction { interaction_id });
    }

    fn validate_and_consume_interaction_response(
        &self,
        thread_id: &str,
        interaction_id: Option<&str>,
    ) -> Result<(), ResumeValidationError> {
        let interaction_id = interaction_id
            .filter(|value| !value.trim().is_empty())
            .ok_or(ResumeValidationError::MissingInteractionId)?;
        let mut pending = self
            .pending_interactions
            .lock()
            .expect("pending interaction lock poisoned");
        let Some(expected) = pending.get(thread_id) else {
            return Err(ResumeValidationError::NoPendingInteraction);
        };
        if expected.interaction_id != interaction_id {
            return Err(ResumeValidationError::InteractionMismatch);
        }
        pending.remove(thread_id);
        Ok(())
    }

    fn history_for_thread(
        &self,
        thread_id: &str,
        request_history: Vec<AgentMessage>,
    ) -> Option<ConversationContext> {
        let mut conversations = self
            .thread_conversations
            .lock()
            .expect("thread conversation lock poisoned");

        if !request_history.is_empty() {
            conversations.insert(
                thread_id.to_string(),
                ThreadConversationState {
                    history: request_history.clone(),
                },
            );
            return Some(ConversationContext::new(request_history));
        }

        conversations.get(thread_id).cloned().and_then(|state| {
            (!state.history.is_empty()).then(|| ConversationContext::new(state.history))
        })
    }

    fn record_completed_turn(
        &self,
        thread_id: &str,
        user_message: AgentMessage,
        assistant_text: &str,
    ) {
        let mut conversations = self
            .thread_conversations
            .lock()
            .expect("thread conversation lock poisoned");
        let state = conversations.entry(thread_id.to_string()).or_default();
        state.history.push(user_message);
        if !assistant_text.trim().is_empty() {
            state.history.push(AgentMessage::new(
                Role::Agent,
                vec![ContentPart::Text(assistant_text.to_string())],
            ));
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResumeValidationError {
    MissingInteractionId,
    NoPendingInteraction,
    InteractionMismatch,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
impl ResumeValidationError {
    fn as_code(self) -> &'static str {
        match self {
            Self::MissingInteractionId => "MISSING_INTERACTION_ID",
            Self::NoPendingInteraction => "NO_PENDING_INTERACTION",
            Self::InteractionMismatch => "INTERACTION_MISMATCH",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::MissingInteractionId => "interactionResponse must include interactionId",
            Self::NoPendingInteraction => {
                "interactionResponse does not match any pending interaction for this thread"
            }
            Self::InteractionMismatch => {
                "interactionResponse interactionId does not match the pending interaction for this thread"
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgUiRunRequest {
    message: String,
    #[serde(default)]
    history: Vec<HistoryItem>,
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    interaction_response: Option<serde_json::Value>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    role: String,
    content: String,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
async fn agui_run(
    State(state): State<Arc<AgUiState>>,
    headers: HeaderMap,
    Json(payload): Json<AgUiRunRequest>,
) -> Response {
    let run_id = format!("run-{}", Uuid::new_v4());
    let thread_id = payload
        .thread_id
        .clone()
        .unwrap_or_else(|| format!("thread-{}", Uuid::new_v4()));
    let message_id = format!("msg-{}", Uuid::new_v4());

    if let Some(response) = payload.interaction_response.as_ref() {
        if let Err(err) = state
            .validate_and_consume_interaction_response(&thread_id, extract_interaction_id(response))
        {
            return agui_validation_error_response(thread_id, run_id, err).into_response();
        }
    }

    let user_message = build_user_message(payload.message, payload.interaction_response);
    let request_history = payload
        .history
        .into_iter()
        .map(history_item_to_agent_message)
        .collect();
    let history = state.history_for_thread(&thread_id, request_history);
    let request_headers =
        Arc::new(protocol_transport_core::sanitize_header_map(&headers).into_map());
    let assistant_text = Arc::new(Mutex::new(String::new()));
    let assistant_text_clone = Arc::clone(&assistant_text);
    let state_for_trace = Arc::clone(&state);
    let thread_id_for_trace = thread_id.clone();
    let user_message_for_trace = user_message.clone();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let trace_stream = match state.agent.run_stream(
        user_message,
        history,
        Some(cancel_flag.clone()),
        Some(request_headers),
    ) {
        Ok(stream) => stream,
        Err(err) => {
            return agent_io_sse_stream(futures::stream::iter(vec![
                AgentIoEvent::RunStarted { thread_id, run_id },
                AgentIoEvent::RunError {
                    message: err.to_string(),
                    code: Some("AGUI_RUNTIME".to_string()),
                },
            ]))
            .into_response();
        }
    };

    let trace_stream = trace_stream.inspect(move |event| match event {
        AgentTraceEvent::ContentDelta { delta } => {
            assistant_text_clone
                .lock()
                .expect("assistant text lock poisoned")
                .push_str(delta);
        }
        AgentTraceEvent::InteractionRequested { request } => {
            state_for_trace.record_pending_interaction(
                thread_id_for_trace.clone(),
                request.interaction_id.clone(),
            );
            let assistant_text = assistant_text_clone
                .lock()
                .expect("assistant text lock poisoned")
                .clone();
            state_for_trace.record_completed_turn(
                &thread_id_for_trace,
                user_message_for_trace.clone(),
                &assistant_text,
            );
        }
        AgentTraceEvent::Completed { text, .. } => {
            let assistant_text = text.clone().unwrap_or_else(|| {
                assistant_text_clone
                    .lock()
                    .expect("assistant text lock poisoned")
                    .clone()
            });
            state_for_trace.record_completed_turn(
                &thread_id_for_trace,
                user_message_for_trace.clone(),
                &assistant_text,
            );
        }
        _ => {}
    });

    ag_ui_sse_response(
        AgUiDriverConfig {
            ctx: IoEventContext {
                thread_id,
                run_id,
                message_id,
            },
            cancel_flag: Some(cancel_flag),
            enrichers: vec![],
        },
        trace_stream,
    )
    .into_response()
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn history_item_to_agent_message(item: HistoryItem) -> AgentMessage {
    let role = if item.role == "assistant" {
        Role::Agent
    } else {
        Role::User
    };
    AgentMessage::new(role, vec![ContentPart::Text(item.content)])
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn build_user_message(
    message: String,
    interaction_response: Option<serde_json::Value>,
) -> AgentMessage {
    let mut parts = vec![ContentPart::Text(message)];
    if let Some(response) = interaction_response {
        parts.push(ContentPart::Data(response));
    }
    AgentMessage::new(Role::User, parts)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn extract_interaction_id(response: &serde_json::Value) -> Option<&str> {
    response
        .get("interactionId")
        .or_else(|| response.get("interaction_id"))
        .and_then(serde_json::Value::as_str)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "event-stream"))]
fn agui_validation_error_response(
    thread_id: String,
    run_id: String,
    err: ResumeValidationError,
) -> Response {
    agent_io_sse_stream(futures::stream::iter(vec![
        AgentIoEvent::RunStarted { thread_id, run_id },
        AgentIoEvent::RunError {
            message: err.message().to_string(),
            code: Some(err.as_code().to_string()),
        },
    ]))
    .into_response()
}
