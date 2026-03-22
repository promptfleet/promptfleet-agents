//! # MCP Proxy
//!
//! **Transport-aware proxy** for connecting to MCP servers
//! - **Client ↔ Proxy**: HTTP/JSON-RPC (internal)
//! - **Proxy ↔ MCP Server**: SSE (web-standard)
//! - **Multi-server routing** with authentication

use crate::{CallToolResult, ListToolsResult, Tool, ToolProvider};
use async_trait::async_trait;
use protocol_transport_core::{
    ProtocolError, SseTransport, Transport, TransportFactory, UniversalRequest,
};
use serde_json::json;
use std::collections::HashMap;

/// **MCP Proxy Configuration**
#[derive(Debug, Clone)]
pub struct McpProxyConfig {
    /// Target MCP servers the proxy routes to
    pub servers: Vec<McpProxyTarget>,
    /// Proxy authentication (for incoming requests)
    pub proxy_auth: Option<String>,
    /// Default timeout for requests (seconds)
    pub timeout_seconds: u64,
}

/// **MCP Proxy Target** - An external MCP server the proxy connects to
#[derive(Debug, Clone)]
pub struct McpProxyTarget {
    /// Server identifier
    pub name: String,
    /// SSE endpoint URL (e.g., "https://api.example.com/sse")
    pub sse_endpoint: String,
    /// Authentication token for this server
    pub auth_token: Option<String>,
    /// Server description
    pub description: Option<String>,
}

/// **MCP Proxy** - Routes between internal HTTP and external SSE
pub struct McpProxy {
    /// Configuration
    config: McpProxyConfig,
    /// SSE transports for external servers
    sse_transports: HashMap<String, SseTransport>,
}

impl McpProxy {
    /// Create new MCP proxy
    pub fn new(config: McpProxyConfig) -> Self {
        // Create SSE transports for each server
        let mut sse_transports = HashMap::new();

        for server in &config.servers {
            let transport = match &server.auth_token {
                Some(token) => TransportFactory::mcp_sse_auth(&server.sse_endpoint, token),
                None => TransportFactory::mcp_sse(&server.sse_endpoint),
            };
            sse_transports.insert(server.name.clone(), transport);
        }

        Self {
            config,
            sse_transports,
        }
    }

    /// Send JSON-RPC request to specific server
    async fn send_to_server(
        &self,
        server_name: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ProtocolError> {
        let transport = self.sse_transports.get(server_name).ok_or_else(|| {
            ProtocolError::internal_error(&format!("Unknown server: {}", server_name))
        })?;

        // Build JSON-RPC request
        let request = UniversalRequest {
            method: method.to_string(),
            uri: "/".to_string(),
            headers: HashMap::new(),
            body: json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
                "id": 1
            })
            .to_string()
            .into_bytes(),
            protocol: "MCP".to_string(),
            correlation_id: format!("{}-{}", method.replace("/", "-"), server_name),
        };

        // Send via SSE transport
        let response = transport
            .send(request)
            .await
            .map_err(|e| ProtocolError::internal_error(&format!("Transport error: {:?}", e)))?;

        // Parse JSON-RPC response
        let response_body = String::from_utf8(response.body)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid UTF-8 response: {}", e)))?;

        let response_json: serde_json::Value = serde_json::from_str(&response_body)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid JSON response: {}", e)))?;

        // Extract result from JSON-RPC
        response_json
            .get("result")
            .ok_or_else(|| ProtocolError::Parsing("Missing 'result' field".to_string()))
            .map(|v| v.clone())
    }

    /// List tools from all servers (async version)
    pub async fn list_tools_async(&self) -> Result<Vec<Tool>, ProtocolError> {
        let mut all_tools = Vec::new();

        for server in &self.config.servers {
            match self
                .send_to_server(&server.name, "tools/list", json!({}))
                .await
            {
                Ok(result) => {
                    let list_result: ListToolsResult =
                        serde_json::from_value(result).map_err(|e| {
                            ProtocolError::Parsing(format!("Invalid tools list format: {}", e))
                        })?;

                    // Prefix tool names with server name for uniqueness
                    let mut tools = list_result.tools;
                    for tool in &mut tools {
                        tool.name = format!("{}:{}", server.name, tool.name);
                    }
                    all_tools.extend(tools);
                }
                Err(e) => {
                    log::warn!(
                        "Failed to list tools from proxy target '{}': {:?}",
                        server.name,
                        e
                    );
                }
            }
        }

        Ok(all_tools)
    }

    /// Call tool (async version)
    pub async fn call_tool_async(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        // Parse tool name: "server:tool" format
        let parts: Vec<&str> = name.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ProtocolError::internal_error(
                "Tool name must be in format 'server:tool'",
            ));
        }

        let server_name = parts[0];
        let tool_name = parts[1];

        let params = json!({
            "name": tool_name,
            "arguments": arguments
        });

        let result = self
            .send_to_server(server_name, "tools/call", params)
            .await?;

        let call_result: CallToolResult = serde_json::from_value(result).map_err(|e| {
            ProtocolError::Parsing(format!("Invalid tool call result format: {}", e))
        })?;

        Ok(call_result)
    }

    /// Health check all servers
    pub async fn health_check_all(&self) -> HashMap<String, bool> {
        let mut health_status = HashMap::new();

        for server in &self.config.servers {
            if let Some(transport) = self.sse_transports.get(&server.name) {
                let is_healthy = transport.health_check().await.is_ok();
                health_status.insert(server.name.clone(), is_healthy);
            } else {
                health_status.insert(server.name.clone(), false);
            }
        }

        health_status
    }
}

#[async_trait]
impl ToolProvider for McpProxy {
    fn list_tools(&self) -> Result<Vec<Tool>, ProtocolError> {
        // Note: Since ToolProvider is sync but SSE calls are async,
        // in practice this would use a cached tool list or async context
        // For now, return placeholder - actual implementation needs async support
        Err(ProtocolError::internal_error(
            "Async tool listing not supported in sync context. Use async proxy methods.",
        ))
    }

    async fn call_tool(
        &self,
        name: &str,
        _arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        let parts: Vec<&str> = name.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ProtocolError::internal_error(
                "Tool name must be in format 'server:tool'",
            ));
        }

        Err(ProtocolError::internal_error(
            "Async tool calls not supported in sync context. Use async proxy methods.",
        ))
    }
}

/// **MCP Proxy Builder** - Convenient proxy configuration
pub struct McpProxyBuilder {
    servers: Vec<McpProxyTarget>,
    proxy_auth: Option<String>,
    timeout_seconds: u64,
}

impl McpProxyBuilder {
    /// Create new proxy builder
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
            proxy_auth: None,
            timeout_seconds: 30,
        }
    }

    /// Add MCP server
    pub fn add_server(mut self, name: &str, sse_endpoint: &str) -> Self {
        self.servers.push(McpProxyTarget {
            name: name.to_string(),
            sse_endpoint: sse_endpoint.to_string(),
            auth_token: None,
            description: None,
        });
        self
    }

    /// Add MCP server with authentication
    pub fn add_server_with_auth(
        mut self,
        name: &str,
        sse_endpoint: &str,
        auth_token: &str,
    ) -> Self {
        self.servers.push(McpProxyTarget {
            name: name.to_string(),
            sse_endpoint: sse_endpoint.to_string(),
            auth_token: Some(auth_token.to_string()),
            description: None,
        });
        self
    }

    /// Set proxy authentication token
    pub fn with_proxy_auth(mut self, auth_token: &str) -> Self {
        self.proxy_auth = Some(auth_token.to_string());
        self
    }

    /// Set timeout for external requests
    pub fn with_timeout(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds;
        self
    }

    /// Build the MCP proxy
    pub fn build(self) -> McpProxy {
        let config = McpProxyConfig {
            servers: self.servers,
            proxy_auth: self.proxy_auth,
            timeout_seconds: self.timeout_seconds,
        };

        McpProxy::new(config)
    }
}

impl Default for McpProxyBuilder {
    fn default() -> Self {
        Self::new()
    }
}
