//! # MCP Server
//!
//! **MCP server implementation** for exposing tools and memory operations
//! - **HTTP Server**: Standard HTTP/JSON-RPC (default)
//! - **SSE Server**: Direct SSE connections (feature: "sse-server")

use crate::{
    AuthHandler, CallToolRequest, CallToolResult, InitializeRequest, InitializeResult,
    ListToolsResult, ServerCapabilities, ServerInfo, Tool, ToolProvider,
};
use protocol_transport_core::{ProtocolError, UniversalRequest, UniversalResponse};
use serde_json::json;
use std::collections::HashMap;

#[cfg(feature = "sse-server")]
use protocol_transport_core::{SseTransport, Transport, TransportFactory};

/// **MCP Server** - Expose tools and capabilities via MCP protocol
pub struct McpServer {
    /// Server capabilities
    capabilities: ServerCapabilities,
    /// Authentication handler
    auth_handler: Option<Box<dyn AuthHandler>>,
    /// Tool provider
    tool_provider: Option<Box<dyn ToolProvider>>,
    /// Server info
    server_info: ServerInfo,

    #[cfg(feature = "sse-server")]
    /// SSE transport for direct client connections
    sse_transport: Option<SseTransport>,

    #[cfg(feature = "sse-server")]
    /// Server bind address
    bind_address: Option<String>,
}

impl McpServer {
    /// Create new MCP server
    pub fn new() -> Self {
        Self {
            capabilities: ServerCapabilities::default(),
            auth_handler: None,
            tool_provider: None,
            server_info: ServerInfo {
                name: "promptfleet-mcp-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("PromptFleet MCP Server".to_string()),
            },

            #[cfg(feature = "sse-server")]
            sse_transport: None,

            #[cfg(feature = "sse-server")]
            bind_address: None,
        }
    }

    /// Configure server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Configure authentication handler
    pub fn with_auth_handler<H: AuthHandler + 'static>(mut self, handler: H) -> Self {
        self.auth_handler = Some(Box::new(handler));
        self
    }

    /// Configure tool provider
    pub fn with_tool_provider<P: ToolProvider + 'static>(mut self, provider: P) -> Self {
        self.tool_provider = Some(Box::new(provider));
        self
    }

    /// Configure server info
    pub fn with_server_info(mut self, server_info: ServerInfo) -> Self {
        self.server_info = server_info;
        self
    }

    /// Bind SSE server to address (feature: "sse-server")
    #[cfg(feature = "sse-server")]
    pub fn with_sse_server(mut self, bind_address: &str) -> Self {
        self.sse_transport = Some(TransportFactory::mcp_sse(bind_address));
        self.bind_address = Some(bind_address.to_string());
        self
    }

    /// Handle incoming MCP request
    pub fn handle_request(
        &self,
        request: &UniversalRequest,
    ) -> Result<UniversalResponse, ProtocolError> {
        // Check authentication if configured
        if let Some(ref auth_handler) = self.auth_handler {
            auth_handler.validate_request(request)?;
        }

        // Parse JSON-RPC request
        let request_body = String::from_utf8(request.body.clone())
            .map_err(|e| ProtocolError::Parsing(format!("Invalid UTF-8 request: {}", e)))?;

        let request_json: serde_json::Value = serde_json::from_str(&request_body)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid JSON request: {}", e)))?;

        let method = request_json
            .get("method")
            .and_then(|m| m.as_str())
            .ok_or_else(|| ProtocolError::Parsing("Missing 'method' field".to_string()))?;

        let params = request_json.get("params").cloned().unwrap_or(json!({}));

        let id = request_json.get("id").cloned();

        // Handle MCP methods
        let response_json = match method {
            "initialize" => self.handle_initialize(params, id)?,
            "tools/list" => self.handle_list_tools(params, id)?,
            "tools/call" => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    tokio::runtime::Handle::current()
                        .block_on(self.handle_call_tool(params, id))?
                }
                #[cfg(target_arch = "wasm32")]
                {
                    return Err(ProtocolError::internal_error(
                        "Sync MCP server handle_request is not supported in WASM; use async handler",
                    ));
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method '{}' not found", method)
                }
            }),
        };

        // Build response
        let response_body = response_json.to_string().into_bytes();
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("x-protocol".to_string(), "MCP".to_string());

        Ok(UniversalResponse {
            status: 200,
            headers,
            body: response_body,
            protocol: "MCP".to_string(),
            correlation_id: request.correlation_id.clone(),
        })
    }

    fn handle_initialize(
        &self,
        params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ProtocolError> {
        let _init_request: InitializeRequest = serde_json::from_value(params)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid initialize request: {}", e)))?;

        let result = InitializeResult {
            protocol_version: crate::MCP_PROTOCOL_VERSION.to_string(),
            capabilities: self.capabilities.clone(),
            server_info: self.server_info.clone(),
        };

        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    fn handle_list_tools(
        &self,
        _params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ProtocolError> {
        let tools = match &self.tool_provider {
            Some(provider) => provider.list_tools()?,
            None => vec![], // No provider configured
        };

        let result = ListToolsResult { tools };

        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    async fn handle_call_tool(
        &self,
        params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ProtocolError> {
        let call_request: CallToolRequest = serde_json::from_value(params)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid call_tool request: {}", e)))?;

        let result = match &self.tool_provider {
            Some(provider) => provider
                .call_tool(&call_request.name, call_request.arguments)
                .await?,
            None => CallToolResult {
                content: vec![crate::Content::text("No tool provider configured")],
                is_error: Some(true),
            },
        };

        Ok(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result
        }))
    }

    /// Start SSE server (feature: "sse-server")
    #[cfg(feature = "sse-server")]
    pub async fn start_sse_server(&self) -> Result<(), ProtocolError> {
        if let Some(ref transport) = self.sse_transport {
            println!(
                "🚀 Starting MCP SSE Server on {}",
                self.bind_address.as_ref().unwrap_or(&"unknown".to_string())
            );

            // This would be the actual server loop in a real implementation
            // For now, we'll just return success
            transport.health_check().await.map_err(|e| {
                ProtocolError::internal_error(&format!("Failed to start SSE server: {:?}", e))
            })
        } else {
            Err(ProtocolError::internal_error("No SSE transport configured"))
        }
    }

    /// Stop SSE server (feature: "sse-server")
    #[cfg(feature = "sse-server")]
    pub async fn stop_sse_server(&self) -> Result<(), ProtocolError> {
        println!("🛑 Stopping MCP SSE Server");
        // Server cleanup would go here
        Ok(())
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

/// **MCP Server Builder** - Convenient server configuration
pub struct McpServerBuilder {
    capabilities: ServerCapabilities,
    auth_handler: Option<Box<dyn AuthHandler>>,
    tool_provider: Option<Box<dyn ToolProvider>>,
    server_info: ServerInfo,

    #[cfg(feature = "sse-server")]
    bind_address: Option<String>,
}

impl McpServerBuilder {
    /// Create new server builder
    pub fn new() -> Self {
        Self {
            capabilities: ServerCapabilities::default(),
            auth_handler: None,
            tool_provider: None,
            server_info: ServerInfo {
                name: "promptfleet-mcp-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("PromptFleet MCP Server".to_string()),
            },

            #[cfg(feature = "sse-server")]
            bind_address: None,
        }
    }

    /// Set server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set authentication handler
    pub fn with_auth_handler<H: AuthHandler + 'static>(mut self, handler: H) -> Self {
        self.auth_handler = Some(Box::new(handler));
        self
    }

    /// Set tool provider
    pub fn with_tool_provider<P: ToolProvider + 'static>(mut self, provider: P) -> Self {
        self.tool_provider = Some(Box::new(provider));
        self
    }

    /// Set server info
    pub fn with_server_info(mut self, server_info: ServerInfo) -> Self {
        self.server_info = server_info;
        self
    }

    /// Bind SSE server to address (feature: "sse-server")
    #[cfg(feature = "sse-server")]
    pub fn with_sse_server(mut self, bind_address: &str) -> Self {
        self.bind_address = Some(bind_address.to_string());
        self
    }

    /// Build the MCP server
    pub fn build(self) -> McpServer {
        let mut server = McpServer::new()
            .with_capabilities(self.capabilities)
            .with_server_info(self.server_info);

        if let Some(handler) = self.auth_handler {
            server.auth_handler = Some(handler);
        }

        if let Some(provider) = self.tool_provider {
            server.tool_provider = Some(provider);
        }

        #[cfg(feature = "sse-server")]
        {
            if let Some(ref address) = self.bind_address {
                server = server.with_sse_server(address);
            }
        }

        server
    }
}

impl Default for McpServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
