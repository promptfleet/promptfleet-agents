//! Inbound: Agent as MCP Server — Interface Only
//!
//! This module defines the [`McpServerAdapter`] trait for exposing agent
//! capabilities as MCP tools. **Concrete implementations are deferred.**
//!
//! # Why this is interface-only
//!
//! All inbound MCP use cases resolve to the same A2A `message/send` call,
//! but the translation logic varies by use case:
//!
//! | Use case | MCP tools/call | Under the hood |
//! |---|---|---|
//! | Skill exposure | `tools/call { name: "analyze_data" }` | A2A `message/send` with skill hint |
//! | LLM tool exposure | `tools/call { name: "get_weather" }` | A2A → LLM tools loop → tool exec |
//! | Free-form chat | `tools/call { name: "ask", args: { prompt } }` | A2A `message/send` with text |
//! | Delegation | `tools/call { name: "plan" }` | A2A `message/send` → full agent run |
//! | Extension exposure | `tools/call { name: "openai.chat" }` | Internal JSON-RPC |
//!
//! Each case has different response semantics (streaming vs sync, artifacts vs
//! text), mapping from `CallToolResult` back to the caller, exposure policy,
//! and auth requirements.
//!
//! # What already exists
//!
//! - `mcp_protocol/server.rs` — MCP server with `ToolProvider` trait
//! - `protocol_transport_core` — `/mcp` route already configured
//! - `agent_sdk` — skill registration, A2A message handling
//!
//! When implementing specific use cases, the concrete `McpServerAdapter`
//! will wire these existing components together.

use crate::mcp_tools::error::McpToolError;

/// Trait for exposing agent capabilities as MCP tools to external MCP clients.
///
/// Implementations will vary by use case:
/// - **SkillExposer**: Registered A2A skills → MCP tools
/// - **ToolExposer**: ToolRegistry entries → MCP tools
/// - **ExtensionExposer**: Distributed-mode extension methods → MCP tools
/// - **ChatExposer**: Single "ask" tool → free-form A2A message/send
///
/// All implementations share:
/// - `list_mcp_tools()` → enumerate exposed capabilities
/// - `handle_mcp_call()` → route to A2A `message/send` (or internal JSON-RPC)
/// - Response mapping → MCP `CallToolResult`
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait McpServerAdapter: Send + Sync {
    /// List capabilities that this adapter exposes as MCP tools.
    ///
    /// Called when an external MCP client sends `tools/list`.
    fn list_mcp_tools(&self) -> Vec<McpToolDefinition>;

    /// Handle an incoming MCP `tools/call`.
    ///
    /// Routes to the appropriate A2A message/send handler or internal executor.
    /// The response is mapped back to an MCP-compatible result.
    async fn handle_mcp_call(
        &self,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<McpCallResponse, McpToolError>;
}

/// MCP tool definition for external clients (matches mcp_protocol::Tool shape).
#[derive(Debug, Clone)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Response from handling an MCP tool call (matches mcp_protocol::CallToolResult shape).
#[derive(Debug, Clone)]
pub struct McpCallResponse {
    /// Text content items
    pub content: Vec<McpCallResponseContent>,
    /// Whether the call resulted in an error
    pub is_error: bool,
}

/// Content item in an MCP call response.
#[derive(Debug, Clone)]
pub enum McpCallResponseContent {
    Text(String),
}
