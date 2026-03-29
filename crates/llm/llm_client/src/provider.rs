//! Internal provider abstraction behind [`crate::LlmClient`].

use crate::error::LlmError;
use crate::model_client::ClientCapabilities;
use crate::stream::LlmEventStream;
use crate::types::{LlmRequest, LlmResponse};
use std::future::Future;
use std::pin::Pin;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type ChatFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub(crate) type ChatFuture<'a> = Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type ChatStreamFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmEventStream, LlmError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub(crate) type ChatStreamFuture<'a> =
    Pin<Box<dyn Future<Output = Result<LlmEventStream, LlmError>> + 'a>>;

/// Wire-format implementation invoked by [`crate::LlmClient`].
pub(crate) trait LlmProvider: Send + Sync {
    fn capabilities(&self) -> ClientCapabilities;

    fn chat<'a>(&'a self, req: LlmRequest) -> ChatFuture<'a>;

    fn chat_stream<'a>(&'a self, req: LlmRequest) -> ChatStreamFuture<'a>;
}
