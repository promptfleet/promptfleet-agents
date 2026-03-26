//! # MCP Tools Integration
//!
//! Bidirectional MCP (Model Context Protocol) integration for PromptFleet agents.
//!
//! ## Outbound (agent uses MCP tools) — implemented
//!
//! Connect to external MCP servers and inject their tools into [`crate::agent::tools::ToolRegistry`]:
//!
//! ```rust,ignore
//! let config = McpServersConfig::from_file("mcp_servers.json")?;
//! let source = NativeMcpBackend::connect(&config).await?;
//! let mut tools = crate::agent::tools::ToolRegistry::new();
//! McpToolAdapter::register_mcp_tools(&mut tools, Arc::new(source)).await?;
//! // tools now contains mcp.<server_id>.<tool_name> entries
//! ```
//!
//! ## Inbound (agent as MCP server) — interface only
//!
//! The [`McpServerAdapter`] trait defines the boundary for exposing agent
//! capabilities as MCP tools. Concrete implementations are deferred to
//! specific use cases (skill exposure, extension exposure, etc.).
//!
//! ## Dual Backend
//!
//! - **Native**: [`NativeMcpBackend`] wraps `rmcp` (stdio + streamable HTTP + OAuth)
//! - **WASM** (`wasm32`): `WasmMcpBackend` wraps `mcp_protocol` (streamable HTTP via spin-sdk)

pub mod adapter;
pub mod config;
pub mod error;
pub mod server_adapter;
pub mod types;
pub mod web_search;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// Re-exports
pub use adapter::McpToolAdapter;
pub use config::{McpServerEntry, McpServersConfig, McpTransportType, ToolPolicy};
pub use error::McpToolError;
pub use server_adapter::McpServerAdapter;
pub use types::{McpCallResult, McpContent, McpToolDescriptor, McpToolSource};
pub use web_search::{
    tool_category, ExtractOptions, ExtractedContent, SearchDepth, SearchOptions, SearchResponse,
    SearchResult, WebSearchError, WebSearchProvider,
};

#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeMcpBackend;

#[cfg(target_arch = "wasm32")]
pub use wasm::WasmMcpBackend;
