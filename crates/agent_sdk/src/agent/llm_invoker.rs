//! LLM invoker traits, provider adapters, and policy configuration.
//!
//! This module defines the object-safe traits for calling LLMs (both
//! request-response and streaming) and the [`LlmPolicy`] struct that governs
//! tool-loop limits, checkpoint behavior, and context-window management.
//!
//! All public items are re-exported through `llm_orchestrator` so that
//! existing import paths (`agent_sdk::agent::llm_orchestrator::LlmPolicy`)
//! remain unchanged.

use crate::runtime_vars::CheckpointMode;
use llm_client::{LlmClient, LlmRequest, LlmResponse};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// WASM-friendly future type: non-Send on wasm32, Send on native
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
type LlmFuture =
    std::pin::Pin<Box<dyn core::future::Future<Output = Result<LlmResponse, String>>>>;
#[cfg(not(target_arch = "wasm32"))]
type LlmFuture = std::pin::Pin<
    Box<dyn core::future::Future<Output = Result<LlmResponse, String>> + Send>,
>;

// ---------------------------------------------------------------------------
// Request-response invoker (WASM + native)
// ---------------------------------------------------------------------------

/// Object-safe invoker to call provider-agnostic LLMs
pub trait LlmInvoker: Send + Sync {
    fn request(&self, req: LlmRequest) -> LlmFuture;
}

/// Conversion trait: let users pass concrete clients directly
pub trait IntoLlmInvoker {
    fn into_invoker(self) -> Arc<dyn LlmInvoker>;
}

impl<T> IntoLlmInvoker for Arc<T>
where
    T: LlmInvoker + 'static,
{
    fn into_invoker(self) -> Arc<dyn LlmInvoker> {
        self as Arc<dyn LlmInvoker>
    }
}

impl IntoLlmInvoker for LlmClient {
    fn into_invoker(self) -> Arc<dyn LlmInvoker> {
        struct C(LlmClient);
        impl LlmInvoker for C {
            fn request(&self, req: LlmRequest) -> LlmFuture {
                let inner = self.0.clone();
                Box::pin(async move { inner.chat(req).await.map_err(|e| e.to_string()) })
            }
        }
        Arc::new(C(self))
    }
}

impl IntoLlmInvoker for Arc<LlmClient> {
    fn into_invoker(self) -> Arc<dyn LlmInvoker> {
        struct C(Arc<LlmClient>);
        impl LlmInvoker for C {
            fn request(&self, req: LlmRequest) -> LlmFuture {
                let inner = self.0.clone();
                Box::pin(async move { inner.chat(req).await.map_err(|e| e.to_string()) })
            }
        }
        Arc::new(C(self))
    }
}

// ---------------------------------------------------------------------------
// Native-only: streaming LLM invoker
// ---------------------------------------------------------------------------

/// Future type for streaming LLM requests (native-only).
///
/// Public so that downstream code (e.g. semaphore wrappers) can implement
/// [`LlmStreamInvoker`] without re-spelling the full pinned box type.
#[cfg(not(target_arch = "wasm32"))]
pub type LlmStreamFuture = std::pin::Pin<
    Box<dyn core::future::Future<Output = Result<llm_client::LlmEventStream, String>> + Send>,
>;

/// Object-safe invoker that returns a streaming event stream.
#[cfg(not(target_arch = "wasm32"))]
pub trait LlmStreamInvoker: Send + Sync {
    fn request_stream(&self, req: LlmRequest) -> LlmStreamFuture;
}

/// Conversion trait: let users pass concrete clients directly.
#[cfg(not(target_arch = "wasm32"))]
pub trait IntoLlmStreamInvoker {
    fn into_stream_invoker(self) -> Arc<dyn LlmStreamInvoker>;
}

#[cfg(not(target_arch = "wasm32"))]
impl IntoLlmStreamInvoker for LlmClient {
    fn into_stream_invoker(self) -> Arc<dyn LlmStreamInvoker> {
        struct S(LlmClient);
        impl LlmStreamInvoker for S {
            fn request_stream(&self, req: LlmRequest) -> LlmStreamFuture {
                let inner = self.0.clone();
                Box::pin(async move { inner.chat_stream(req).await.map_err(|e| e.to_string()) })
            }
        }
        Arc::new(S(self))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl IntoLlmStreamInvoker for Arc<LlmClient> {
    fn into_stream_invoker(self) -> Arc<dyn LlmStreamInvoker> {
        struct S(Arc<LlmClient>);
        impl LlmStreamInvoker for S {
            fn request_stream(&self, req: LlmRequest) -> LlmStreamFuture {
                let inner = self.0.clone();
                Box::pin(async move { inner.chat_stream(req).await.map_err(|e| e.to_string()) })
            }
        }
        Arc::new(S(self))
    }
}

// ---------------------------------------------------------------------------
// Request defaults (model-aware)
// ---------------------------------------------------------------------------

/// Default request parameters applied to every LLM turn.
///
/// These are merged into the JSON payload before sending. When a
/// [`ModelConfig`](llm_client::ModelConfig) is provided, the values are
/// sanitized through the `prepare_request` pipeline — e.g. GPT-5 gets
/// temperature stripped and max_tokens remapped to max_completion_tokens.
#[derive(Debug, Clone, Default)]
pub struct LlmRequestDefaults {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Extra top-level JSON fields (e.g. reasoning_effort, chat_template_kwargs).
    /// These are merged last and can override anything.
    pub extensions: Option<serde_json::Map<String, serde_json::Value>>,
    /// Model profile for per-model mutation/validation (GPT-5, Qwen3, etc.).
    /// When present, the orchestrator runs `prepare_request` to sanitize fields.
    pub model_config: Option<llm_client::ModelConfig>,
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// Minimal policy for MVP
#[derive(Debug, Clone)]
pub struct LlmPolicy {
    pub finalize_required: bool,
    pub max_failed_tool_calls: usize,
    // New decoupled limits
    pub max_turns: Option<usize>,
    pub max_tool_calls: Option<usize>,
    pub max_no_progress_turns: Option<usize>,
    pub wall_clock_timeout_ms: Option<u64>,
    // checkpoint_task gating (runtime-configurable)
    pub checkpoint_mode: CheckpointMode,
    pub checkpoint_allow_message_response: bool,
    pub checkpoint_mirror_internal_state_to_task_meta: bool,
    /// Maximum tokens for the context window (input messages + tools).
    ///
    /// When set, the orchestrator trims the message history using a sliding
    /// window before each LLM call to stay within this budget. Derived from
    /// [`ModelCapabilities::context_window`] minus output reservation.
    ///
    /// When `None`, messages accumulate unboundedly (legacy behavior).
    pub max_context_tokens: Option<u32>,
}

impl Default for LlmPolicy {
    fn default() -> Self {
        Self {
            finalize_required: true,
            max_failed_tool_calls: 5,
            max_turns: None,
            max_tool_calls: None,
            max_no_progress_turns: Some(3),
            wall_clock_timeout_ms: Some(120_000),
            checkpoint_mode: CheckpointMode::TaskObservable,
            checkpoint_allow_message_response: false,
            checkpoint_mirror_internal_state_to_task_meta: true,
            max_context_tokens: None,
        }
    }
}
