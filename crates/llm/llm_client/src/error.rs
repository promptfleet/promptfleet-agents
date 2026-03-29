//! Unified error type for LLM HTTP, protocol, and serialization failures.

use protocol_transport_core::{ProtocolError, TransportError};

/// Errors surfaced by [`crate::LlmClient`] and streaming helpers.
#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("invalid configuration: {0}")]
    Config(String),
}

pub type LlmResult<T> = Result<T, LlmError>;
