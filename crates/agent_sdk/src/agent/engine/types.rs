//! Public types for the ToolEngine: config, result, error, turn abstraction.

use crate::agent::llm_invoker::LlmRequestDefaults;
use crate::agent::tools::ToolRegistry;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crate::agent::llm_invoker::LlmStreamInvoker;
#[cfg(not(target_arch = "wasm32"))]
use crate::agent::trace::{AgentTraceEvent, AgentTraceStream};

// ===========================================================================
// Turn abstraction — the key decoupling layer
// ===========================================================================

/// Result of a single LLM turn (streaming or request-response).
///
/// The core loop sees only this — it doesn't know whether the turn was
/// serviced by a streaming provider or a synchronous JSON endpoint.
#[derive(Debug, Clone)]
pub struct TurnResult {
    /// Accumulated text content from the assistant.
    pub content: String,
    /// Tool calls requested by the model (may be empty).
    pub tool_calls: Vec<ToolCallInfo>,
    /// Provider finish reason: `"stop"`, `"tool_calls"`, `"length"`, etc.
    pub finish_reason: Option<String>,
    /// Token usage statistics (if reported by the provider).
    pub usage: Option<llm_client::Usage>,
}

/// A single tool call extracted from an LLM turn.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    /// Index within the turn (for parallel tool calls).
    pub index: u32,
    /// Provider-assigned tool call ID (e.g. `"call_abc123"`).
    pub id: String,
    /// Function name.
    pub name: String,
    /// Raw JSON arguments string (not yet parsed).
    pub arguments_raw: String,
}

// ---------------------------------------------------------------------------
// TurnFuture — conditional Send for WASM vs native
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
pub type TurnFuture =
    std::pin::Pin<Box<dyn core::future::Future<Output = Result<TurnResult, EngineError>>>>;
#[cfg(not(target_arch = "wasm32"))]
pub type TurnFuture =
    std::pin::Pin<Box<dyn core::future::Future<Output = Result<TurnResult, EngineError>> + Send>>;

/// Object-safe trait abstracting a single LLM turn.
///
/// Two implementations:
/// - **`StreamingTurnInvoker`** (native-only): wraps `LlmStreamInvoker`,
///   accumulates stream deltas into `TurnResult`, emits `ContentDelta`/
///   `ReasoningDelta` events via a captured sink.
/// - **`RequestResponseTurnInvoker`** (WASM + native): wraps `LlmInvoker`,
///   parses the JSON response into `TurnResult`.
///
/// The core loop calls `invoke_turn()` and processes the result uniformly.
pub trait LlmTurnInvoker: Send + Sync {
    fn invoke_turn(&self, request: llm_client::LlmRequest) -> TurnFuture;
}

// ===========================================================================
// EngineConfig
// ===========================================================================

/// Configuration for the tool-calling execution loop.
///
/// Contains only execution-control concerns — no protocol-specific fields.
/// For sentinel-tool finalization behavior (checkpoint modes, finalize), see
/// [`LlmPolicy`](crate::agent::llm_invoker::LlmPolicy).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum LLM turns before the loop stops. `None` = unlimited. Default: 10.
    pub max_turns: Option<usize>,
    /// Maximum total tool calls across all turns. `None` = unlimited.
    pub max_tool_calls: Option<usize>,
    /// Wall-clock timeout in milliseconds. `None` = no timeout.
    pub wall_clock_timeout_ms: Option<u64>,
    /// Token budget for the context window (messages + tools).
    /// When set, the engine trims message history before each LLM call.
    /// `None` = unbounded accumulation.
    pub max_context_tokens: Option<u32>,
    /// Per-request LLM defaults (temperature, max_tokens, model-specific extensions).
    pub request_defaults: Option<LlmRequestDefaults>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_turns: Some(10),
            max_tool_calls: None,
            wall_clock_timeout_ms: Some(120_000),
            max_context_tokens: None,
            request_defaults: None,
        }
    }
}

// ===========================================================================
// EngineResult
// ===========================================================================

/// Successful result from a ToolEngine execution.
#[derive(Debug, Clone)]
pub struct EngineResult {
    /// The final accumulated text from the assistant. `None` if the model
    /// produced no text (e.g. only tool calls before hitting a limit).
    pub text: Option<String>,
    /// Token usage statistics from the last LLM turn (if reported).
    pub usage: Option<llm_client::Usage>,
    /// Number of LLM turns executed.
    pub turns_used: u32,
    /// Total number of tool calls executed across all turns.
    pub tool_calls_made: usize,
    /// If a sentinel tool (e.g. `checkpoint_task`) signalled stop,
    /// its output is captured here for the adapter to interpret.
    pub stop_signal: Option<serde_json::Value>,
}

// ===========================================================================
// EngineError
// ===========================================================================

/// Typed errors from the ToolEngine execution loop.
#[derive(Debug, Clone)]
pub enum EngineError {
    /// Execution was cancelled by the caller or stream consumer.
    Cancelled,
    /// The LLM provider returned an error or the stream failed.
    LlmFailed(String),
    /// The turn limit was reached without a final text response.
    TurnLimit { turns: u32 },
    /// The tool-call limit was reached.
    ToolCallLimit { count: usize },
    /// The wall-clock timeout was exceeded.
    Timeout { elapsed_ms: u64 },
    /// The LLM returned an empty response with no tool calls.
    EmptyResponse,
    /// A stream-level error occurred.
    StreamError(String),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "Execution cancelled"),
            Self::LlmFailed(msg) => write!(f, "LLM request failed: {}", msg),
            Self::TurnLimit { turns } => write!(f, "Turn limit reached ({})", turns),
            Self::ToolCallLimit { count } => write!(f, "Tool call limit reached ({})", count),
            Self::Timeout { elapsed_ms } => write!(f, "Timeout after {}ms", elapsed_ms),
            Self::EmptyResponse => write!(f, "LLM returned empty response"),
            Self::StreamError(msg) => write!(f, "Stream error: {}", msg),
        }
    }
}

impl std::error::Error for EngineError {}

// ===========================================================================
// ToolEngine (native-only convenience wrapper)
// ===========================================================================

/// Protocol-agnostic tool-augmented LLM execution engine.
///
/// Owns an LLM invoker, a tool registry, and configuration. Provides
/// multiple entry points (`run_text`, `run_stream`, `execute`) that all
/// share the same core loop implementation.
#[cfg(not(target_arch = "wasm32"))]
pub struct ToolEngine {
    pub(crate) llm: Arc<dyn LlmStreamInvoker>,
    pub(crate) model: String,
    pub(crate) tools: ToolRegistry,
    pub(crate) config: EngineConfig,
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for ToolEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolEngine")
            .field("model", &self.model)
            .field("tools_count", &self.tools.len())
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ToolEngine {
    pub fn new(
        llm: Arc<dyn LlmStreamInvoker>,
        model: impl Into<String>,
        tools: ToolRegistry,
        config: EngineConfig,
    ) -> Self {
        Self {
            llm,
            model: model.into(),
            tools,
            config,
        }
    }

    pub fn builder() -> ToolEngineBuilder {
        ToolEngineBuilder::default()
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_config(mut self, config: EngineConfig) -> Self {
        self.config = config;
        self
    }

    // ── Convenience entry points ─────────────────────────────────────

    pub async fn run_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<EngineResult, EngineError> {
        let messages = build_messages(Some(system_prompt), user_prompt, None);
        self.run_messages(messages).await
    }

    pub async fn run_with_history(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        history: &[(&str, &str)],
    ) -> Result<EngineResult, EngineError> {
        let messages = build_messages(Some(system_prompt), user_prompt, Some(history));
        self.run_messages(messages).await
    }

    pub fn run_stream(&self, system_prompt: &str, user_prompt: &str) -> AgentTraceStream {
        let messages = build_messages(Some(system_prompt), user_prompt, None);
        self.run_messages_stream(messages)
    }

    pub fn run_stream_with_history(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        history: &[(&str, &str)],
    ) -> AgentTraceStream {
        let messages = build_messages(Some(system_prompt), user_prompt, Some(history));
        self.run_messages_stream(messages)
    }

    // ── Core entry points (pre-built messages) ───────────────────────

    pub async fn run_messages(
        &self,
        mut messages: Vec<llm_client::ChatMessage>,
    ) -> Result<EngineResult, EngineError> {
        use super::invokers::StreamingTurnInvoker;

        let noop_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(|_| {});
        let invoker = StreamingTurnInvoker::new(self.llm.clone(), noop_sink);
        super::core_loop::execute(
            &invoker,
            &self.model,
            &self.tools,
            &self.config,
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await
    }

    pub fn run_messages_stream(
        &self,
        mut messages: Vec<llm_client::ChatMessage>,
    ) -> AgentTraceStream {
        use super::invokers::StreamingTurnInvoker;

        let (tx, rx) = tokio::sync::mpsc::channel::<AgentTraceEvent>(64);
        let tx_for_delta = tx.clone();
        let tx_for_tools = tx.clone();
        let delta_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
            let _ = tx_for_delta.try_send(event);
        });
        let invoker = StreamingTurnInvoker::new(self.llm.clone(), delta_sink);

        let model = self.model.clone();
        let tools = self.tools.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let on_event = |event: AgentTraceEvent| {
                let _ = tx.try_send(event);
            };
            let event_sink: Arc<dyn Fn(AgentTraceEvent) + Send + Sync> = Arc::new(move |event| {
                let _ = tx_for_tools.try_send(event);
            });
            let _ = super::core_loop::execute(
                &invoker,
                &model,
                &tools,
                &config,
                &mut messages,
                &on_event,
                Some(event_sink),
                None,
                None,
            )
            .await;
        });

        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        }))
    }
}

// ===========================================================================
// Builder
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct ToolEngineBuilder {
    llm: Option<Arc<dyn LlmStreamInvoker>>,
    model: Option<String>,
    tools: Option<ToolRegistry>,
    max_turns: Option<usize>,
    max_tool_calls: Option<usize>,
    wall_clock_timeout_ms: Option<u64>,
    max_context_tokens: Option<u32>,
    request_defaults: Option<LlmRequestDefaults>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ToolEngineBuilder {
    pub fn llm(mut self, llm: Arc<dyn LlmStreamInvoker>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = Some(tools);
        self
    }

    pub fn max_turns(mut self, n: usize) -> Self {
        self.max_turns = Some(n);
        self
    }

    pub fn max_tool_calls(mut self, n: usize) -> Self {
        self.max_tool_calls = Some(n);
        self
    }

    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.wall_clock_timeout_ms = Some(ms);
        self
    }

    pub fn max_context_tokens(mut self, tokens: u32) -> Self {
        self.max_context_tokens = Some(tokens);
        self
    }

    pub fn request_defaults(mut self, defaults: LlmRequestDefaults) -> Self {
        self.request_defaults = Some(defaults);
        self
    }

    pub fn build(self) -> Result<ToolEngine, String> {
        let llm = self.llm.ok_or("ToolEngineBuilder: `llm` is required")?;
        let model = self.model.ok_or("ToolEngineBuilder: `model` is required")?;

        let config = EngineConfig {
            max_turns: self.max_turns.or(Some(10)),
            max_tool_calls: self.max_tool_calls,
            wall_clock_timeout_ms: self.wall_clock_timeout_ms.or(Some(120_000)),
            max_context_tokens: self.max_context_tokens,
            request_defaults: self.request_defaults,
        };

        Ok(ToolEngine {
            llm,
            model,
            tools: self.tools.unwrap_or_default(),
            config,
        })
    }
}

// ===========================================================================
// Message builders (shared helpers)
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
fn build_messages(
    system_prompt: Option<&str>,
    user_prompt: &str,
    history: Option<&[(&str, &str)]>,
) -> Vec<llm_client::ChatMessage> {
    let mut messages = Vec::new();

    if let Some(sys) = system_prompt {
        if !sys.is_empty() {
            messages.push(llm_client::ChatMessage {
                role: "system".into(),
                content: Some(sys.into()),
                ..Default::default()
            });
        }
    }

    if let Some(turns) = history {
        for (role, text) in turns {
            if !text.is_empty() {
                messages.push(llm_client::ChatMessage {
                    role: (*role).to_string(),
                    content: Some((*text).into()),
                    ..Default::default()
                });
            }
        }
    }

    messages.push(llm_client::ChatMessage {
        role: "user".into(),
        content: Some(user_prompt.into()),
        ..Default::default()
    });
    messages
}
