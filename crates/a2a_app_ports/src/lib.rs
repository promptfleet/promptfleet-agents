//! Application Ports for A2A HTTP Server (v1.0)
//!
//! Defines the application-facing port that infrastructure (HTTP server)
//! uses to delegate A2A methods to the SDK/application layer.

use a2a_protocol_core::A2AResult;
use a2a_protocol_core::agent::AgentCard;
use a2a_protocol_core::methods::params::{SendMessageRequest, SendMessageResponse};
use std::pin::Pin;

#[cfg(target_arch = "wasm32")]
pub type AppFuture<'a> = Pin<Box<dyn Future<Output = A2AResult<SendMessageResponse>> + 'a>>;
#[cfg(not(target_arch = "wasm32"))]
pub type AppFuture<'a> = Pin<Box<dyn Future<Output = A2AResult<SendMessageResponse>> + Send + 'a>>;

/// Synchronous application port for A2A servers.
pub trait A2AAppPort: Send + Sync {
    fn build_agent_card(&self) -> AgentCard;
    fn handle_send_message(&self, params: SendMessageRequest) -> A2AResult<SendMessageResponse>;
}

/// Async variant of the application port.
pub trait A2AAppPortAsync: Send + Sync {
    fn build_agent_card(&self) -> AgentCard;
    fn handle_send_message_async<'a>(&'a self, params: SendMessageRequest) -> AppFuture<'a>;
}
