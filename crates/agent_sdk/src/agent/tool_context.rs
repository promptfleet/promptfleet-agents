use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::trace::AgentTraceEvent;

/// Execution context passed to context-aware tool executors.
///
/// This is the **session context** that binds tool executions to a request.
/// The LLM never sees it — it only sees tool name/description/schema.
/// When the LLM calls a tool, the executor closure receives `(args, ctx)`
/// where `ctx: ToolContext` carries per-request state.
#[derive(Clone)]
pub struct ToolContext {
    event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
    cancelled: Arc<AtomicBool>,
    request_headers: Arc<HashMap<String, String>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("is_cancelled", &self.is_cancelled())
            .field("request_headers_count", &self.request_headers.len())
            .finish()
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            event_sink: Arc::new(|_| {}),
            cancelled: Arc::new(AtomicBool::new(false)),
            request_headers: Arc::new(HashMap::new()),
        }
    }
}

impl ToolContext {
    pub fn new(
        event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
        cancelled: Arc<AtomicBool>,
    ) -> Self {
        Self {
            event_sink,
            cancelled,
            request_headers: Arc::new(HashMap::new()),
        }
    }

    /// Create a context with per-request headers for transparent propagation.
    pub fn with_headers(
        event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync>,
        cancelled: Arc<AtomicBool>,
        request_headers: Arc<HashMap<String, String>>,
    ) -> Self {
        Self {
            event_sink,
            cancelled,
            request_headers,
        }
    }

    /// Emit a trace event from inside tool execution.
    pub fn emit(&self, event: AgentTraceEvent) {
        (self.event_sink)(event);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Per-request headers propagated from the inbound HTTP request.
    ///
    /// Contains auth tokens, trace IDs, and other transparent headers that
    /// should be forwarded to outbound calls (sub-agents, discovery, etc.).
    pub fn request_headers(&self) -> &HashMap<String, String> {
        &self.request_headers
    }
}
