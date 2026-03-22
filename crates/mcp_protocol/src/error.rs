//! # MCP Protocol Errors
//!
//! **Comprehensive error handling** for MCP protocol operations
//! - JSON-RPC 2.0 compliant error codes
//! - Integration with protocol_transport_core error system
//! - Authentication and authorization error handling

use protocol_transport_core::{ProtocolError, TransportError};
use thiserror::Error;

/// **MCP-specific errors**
#[derive(Error, Debug)]
pub enum McpError {
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Authorization failed: {0}")]
    Authorization(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool execution failed: {0}")]
    ToolExecutionFailed(String),

    #[error("Memory operation failed: {0}")]
    MemoryOperationFailed(String),

    #[error("Invalid tool parameters: {0}")]
    InvalidToolParameters(String),

    #[error("Protocol version mismatch: expected {expected}, got {actual}")]
    ProtocolVersionMismatch { expected: String, actual: String },

    #[error("Server capability not supported: {0}")]
    UnsupportedCapability(String),

    #[error("Request timeout: {0}")]
    Timeout(String),

    #[error("External MCP server error: {0}")]
    ExternalServerError(String),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl McpError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            McpError::Timeout(_) => true,
            McpError::ExternalServerError(_) => true,
            McpError::Transport(te) => te.is_retryable(),
            McpError::Protocol(pe) => pe.is_retryable(),
            _ => false,
        }
    }

    /// Check if error is due to authentication
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            McpError::Authentication(_) | McpError::Authorization(_)
        )
    }

    /// Convert to JSON-RPC error code
    pub fn to_jsonrpc_code(&self) -> i32 {
        match self {
            McpError::JsonRpc(_) => -32603,
            McpError::Authentication(_) => -32001,
            McpError::Authorization(_) => -32002,
            McpError::ToolNotFound(_) => -32601,
            McpError::ToolExecutionFailed(_) => -32603,
            McpError::MemoryOperationFailed(_) => -32603,
            McpError::InvalidToolParameters(_) => -32602,
            McpError::ProtocolVersionMismatch { .. } => -32600,
            McpError::UnsupportedCapability(_) => -32601,
            McpError::Timeout(_) => -32003,
            McpError::ExternalServerError(_) => -32004,
            McpError::Transport(_) => -32603,
            McpError::Protocol(_) => -32603,
            McpError::Serialization(_) => -32700,
        }
    }
}

/// **Result type for MCP operations**
pub type McpResult<T> = Result<T, McpError>;

/// **Convert McpError to ProtocolError**
impl From<McpError> for ProtocolError {
    fn from(err: McpError) -> Self {
        match err {
            McpError::Transport(te) => ProtocolError::Transport(te),
            McpError::Protocol(pe) => pe,
            McpError::Serialization(se) => ProtocolError::Serialization(se),
            other => ProtocolError::Internal(other.to_string()),
        }
    }
}
