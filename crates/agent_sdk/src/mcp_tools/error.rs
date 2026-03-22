//! MCP tool integration errors.

use std::fmt;

/// Errors from MCP tool operations.
#[derive(Debug)]
pub enum McpToolError {
    /// MCP server not found by server_id
    ServerNotFound(String),
    /// Failed to list tools from a server
    ListToolsFailed(String),
    /// Failed to call a tool
    CallToolFailed(String),
    /// Config parsing or loading error
    ConfigError(String),
    /// Connection/transport error
    ConnectionError(String),
    /// Server process failed to start (stdio transport)
    ProcessSpawnFailed(String),
}

impl fmt::Display for McpToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerNotFound(id) => write!(f, "MCP server not found: {}", id),
            Self::ListToolsFailed(e) => write!(f, "Failed to list MCP tools: {}", e),
            Self::CallToolFailed(e) => write!(f, "MCP tool call failed: {}", e),
            Self::ConfigError(e) => write!(f, "MCP config error: {}", e),
            Self::ConnectionError(e) => write!(f, "MCP connection error: {}", e),
            Self::ProcessSpawnFailed(e) => write!(f, "MCP server process failed: {}", e),
        }
    }
}

impl std::error::Error for McpToolError {}
