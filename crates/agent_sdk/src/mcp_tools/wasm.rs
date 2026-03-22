//! WASM MCP backend using `mcp_protocol::McpClient` over Streamable HTTP.
//!
//! This backend is compiled only on `target_arch = "wasm32"` and uses
//! PromptFleet's `mcp_protocol` crate for lightweight MCP communication
//! in SpinKube environments.
//!
//! **Supported transports:**
//! - Remote URL (Streamable HTTP via `spin-sdk::http::send`) — the only
//!   transport available in WASM. Stdio is not supported.
//!
//! **Config:**
//! ```json
//! {
//!   "mcp_servers": {
//!     "tavily": {
//!       "url": "https://mcp.tavily.com/mcp/?tavilyApiKey=...",
//!       "auth_token": "${TAVILY_AUTH_TOKEN}"   // optional
//!     }
//!   }
//! }
//! ```

use std::collections::HashMap;

use mcp_protocol::McpClient;

use crate::mcp_tools::config::{resolve_env_vars, McpServersConfig, McpTransportType};
use crate::mcp_tools::error::McpToolError;
use crate::mcp_tools::types::{
    build_forwarded_headers_meta, McpCallResult, McpContent, McpToolDescriptor, McpToolSource,
};

/// Handle wrapping a single `McpClient` connection.
struct WasmClientHandle {
    server_id: String,
    url: String,
    auth_token: Option<String>,
    forward_caller_auth: bool,
    client: McpClient,
}

impl WasmClientHandle {
    /// List tools from this server.
    async fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, McpToolError> {
        let tools = self
            .client
            .list_tools_async()
            .await
            .map_err(|e| McpToolError::ListToolsFailed(format!("{}: {}", self.server_id, e)))?;

        Ok(tools
            .into_iter()
            .map(|tool| McpToolDescriptor {
                server_id: self.server_id.clone(),
                name: tool.name,
                description: Some(tool.description),
                input_schema: tool.input_schema,
            })
            .collect())
    }

    /// Call a tool on this server.
    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError> {
        self.call_tool_with_headers(name, args, None).await
    }

    async fn call_tool_with_headers(
        &self,
        name: &str,
        args: serde_json::Value,
        request_headers: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let client = self.build_client(request_headers).await?;
        let meta = build_forwarded_headers_meta(
            request_headers,
            self.forward_caller_auth && self.auth_token.is_none(),
        )
        .map(serde_json::Value::Object);
        let result = client
            .call_tool_with_meta_async(name, Some(args), meta)
            .await
            .map_err(|e| {
                McpToolError::CallToolFailed(format!("{}::{}: {}", self.server_id, name, e))
            })?;

        let content = result
            .content
            .into_iter()
            .map(|c| match c {
                mcp_protocol::Content::Text { text } => McpContent::Text(text),
            })
            .collect();

        Ok(McpCallResult {
            content,
            is_error: result.is_error.unwrap_or(false),
        })
    }

    async fn build_client(
        &self,
        request_headers: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<McpClient, McpToolError> {
        let mut client = match &self.auth_token {
            Some(token) => McpClient::new()
                .with_streamable_http_server_auth(&self.url, token),
            None => McpClient::new().with_streamable_http_server(&self.url),
        };

        let mut forwarded = protocol_transport_core::sanitize_headers(
            request_headers.unwrap_or(&std::collections::HashMap::new()),
        )
        .into_map();
        forwarded.retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
        forwarded.retain(|name, _| !name.eq_ignore_ascii_case("mcp-session-id"));

        if !forwarded.is_empty() {
            client = client.with_streamable_http_headers(forwarded);
        }

        client.initialize_async().await.map_err(|e| {
            McpToolError::ConnectionError(format!(
                "{}: streamable HTTP initialize failed: {}",
                self.server_id, e
            ))
        })?;

        Ok(client)
    }
}

// ── Public backend ──────────────────────────────────────────────────────────

/// WASM MCP backend using `mcp_protocol::McpClient`.
///
/// Connects to remote MCP servers via Streamable HTTP using `spin-sdk`.
/// Only URL-based (Remote) transport is supported — stdio entries are
/// skipped with a warning since child processes cannot be spawned in WASM.
///
/// | Transport | Config | Support |
/// |-----------|--------|---------|
/// | Remote (Streamable HTTP) | `url` | Supported |
/// | Stdio (child process) | `command` + `args` | Not supported in WASM |
pub struct WasmMcpBackend {
    servers: HashMap<String, WasmClientHandle>,
}

impl WasmMcpBackend {
    /// Connect to all enabled MCP servers from config.
    ///
    /// Only servers with a `url` field are connected. Servers with `command`
    /// (stdio transport) are skipped with a warning — stdio is not available
    /// in WASM environments.
    ///
    /// Servers that fail to connect log a warning and are skipped —
    /// partial availability is better than total failure.
    pub async fn connect(config: &McpServersConfig) -> Result<Self, McpToolError> {
        let mut servers = HashMap::new();

        for (server_id, entry) in config.enabled_servers() {
            match entry.transport_type() {
                McpTransportType::Stdio => {
                    log::warn!(
                        "MCP server '{}': stdio transport not supported in WASM, skipping \
                         (use 'url' for remote transport instead)",
                        server_id
                    );
                    continue;
                }
                McpTransportType::Remote => {
                    let url = entry.url.as_deref().ok_or_else(|| {
                        McpToolError::ConfigError(format!(
                            "Server '{}': remote transport requires 'url'",
                            server_id
                        ))
                    })?;

                    match Self::connect_remote(
                        server_id,
                        url,
                        entry.auth_token.as_deref(),
                        entry.forward_caller_auth,
                    )
                    .await {
                        Ok(handle) => {
                            log::info!(
                                "MCP server '{}' connected (streamable_http: {})",
                                server_id,
                                url
                            );
                            servers.insert(server_id.clone(), handle);
                        }
                        Err(e) => {
                            log::warn!(
                                "MCP server '{}' connection failed (skipping): {}",
                                server_id,
                                e
                            );
                        }
                    }
                }
                McpTransportType::Unknown => {
                    log::warn!(
                        "MCP server '{}': no 'command' or 'url' specified, skipping",
                        server_id
                    );
                    continue;
                }
            }
        }

        if servers.is_empty() && config.enabled_servers().count() > 0 {
            log::warn!("No MCP servers connected successfully (WASM backend)");
        }

        Ok(Self { servers })
    }

    /// Connect to a single remote MCP server via Streamable HTTP.
    async fn connect_remote(
        server_id: &str,
        url: &str,
        auth_token: Option<&str>,
        forward_caller_auth: bool,
    ) -> Result<WasmClientHandle, McpToolError> {
        let resolved_url = resolve_env_vars(url);
        let resolved_auth_token = auth_token.map(resolve_env_vars);

        let client = match &resolved_auth_token {
            Some(token) => {
                McpClient::new()
                    .with_streamable_http_server_auth(&resolved_url, token)
            }
            None => McpClient::new().with_streamable_http_server(&resolved_url),
        };

        client.initialize_async().await.map_err(|e| {
            McpToolError::ConnectionError(format!(
                "{}: streamable HTTP initialize failed: {}",
                server_id, e
            ))
        })?;

        Ok(WasmClientHandle {
            server_id: server_id.to_string(),
            url: resolved_url,
            auth_token: resolved_auth_token,
            forward_caller_auth,
            client,
        })
    }
}

#[async_trait::async_trait(?Send)]
impl McpToolSource for WasmMcpBackend {
    async fn list_tools(&self, server_id: &str) -> Result<Vec<McpToolDescriptor>, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;
        handle.list_tools().await
    }

    async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;
        handle.call_tool(tool_name, args).await
    }

    async fn call_tool_with_headers(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        request_headers: Option<&HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;
        handle.call_tool_with_headers(tool_name, args, request_headers).await
    }

    async fn health_check(&self, server_id: &str) -> Result<bool, McpToolError> {
        let handle = self
            .servers
            .get(server_id)
            .ok_or_else(|| McpToolError::ServerNotFound(server_id.to_string()))?;

        match handle.client.health_check().await {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn server_ids(&self) -> Vec<String> {
        self.servers.keys().cloned().collect()
    }
}
