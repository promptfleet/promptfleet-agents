//! Provider abstraction behind [`crate::LlmClient`].
//!
//! This trait is intentionally provider- and runtime-agnostic. Concrete
//! implementations own a wire format such as OpenAI-compatible, Anthropic
//! Messages, Vertex AI, or an internal gateway. Platform-specific credential
//! exchange should happen inside the implementation, not in request payloads.

use crate::error::LlmError;
use crate::model_client::ClientCapabilities;
use crate::stream::LlmEventStream;
use crate::types::{LlmRequest, LlmResponse};
use std::future::Future;
use std::pin::Pin;

#[cfg(not(target_arch = "wasm32"))]
pub type ChatFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type ChatFuture<'a> = Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub type ChatStreamFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmEventStream, LlmError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type ChatStreamFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmEventStream, LlmError>> + 'a>>;

/// Wire-format implementation invoked by [`crate::LlmClient`].
///
/// Implement this when the standard [`crate::client::WireFormat`] builders are
/// not enough, for example a model provider that resolves short-lived
/// credentials internally before calling its upstream API.
pub trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ClientCapabilities;

    fn chat<'a>(&'a self, req: LlmRequest) -> ChatFuture<'a>;

    fn chat_stream<'a>(&'a self, req: LlmRequest) -> ChatStreamFuture<'a>;
}
