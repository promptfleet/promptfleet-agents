//! # Model Context Protocol (MCP) Implementation
//!
//! **Standards-compliant MCP implementation** for PromptFleet agents
//! - **JSON-RPC 2.0** wire format (MCP specification requirement)
//! - **HTTP/HTTPS transport** with OAuth 2.1 Bearer token support
//! - **FastMCP compatible** - interoperable with Python FastMCP and TypeScript MCP SDKs
//! - **External MCP servers** - connect to any MCP server via proxy
//!
//! ## Compatibility Verified (June 2025)
//! - MCP Specification 2025-06-18 (latest)
//! - JSON-RPC 2.0 specification compliant
//! - FastMCP Python implementation compatible
//! - Standard OAuth 2.1 Bearer token authentication
//! - HTTP+SSE and Streamable HTTP transport support
//!
//! ## Core Features
//! - **Tool Discovery**: `list_tools` method
//! - **Tool Execution**: `call_tool` method with proper parameter handling
//! - **Memory Operations**: `add_memory`, `search_memory` methods
//! - **Health Monitoring**: `heartbeat` and health endpoints
//! - **Authentication**: Bearer token and custom auth strategies
//! - **Proxy Support**: Connect to external MCP servers outside Kubernetes

pub mod auth;
pub mod client;
pub mod error;
pub mod handler;
pub mod server;
pub mod types;

#[cfg(feature = "proxy")]
pub mod proxy;

pub use error::*;
pub use types::*;

#[cfg(feature = "client")]
pub use client::*;

#[cfg(feature = "server")]
pub use server::*;

pub use auth::*;

#[cfg(feature = "proxy")]
pub use proxy::*;

use async_trait::async_trait;
use protocol_transport_core::{
    AsyncProtocolHandler, ProtocolError, ProtocolHandler, UniversalRequest, UniversalResponse,
};
use std::collections::HashMap;

/// **MCP Protocol Version** - Current specification version
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// **JSON-RPC Version** - Required by MCP specification
pub const JSONRPC_VERSION: &str = "2.0";

/// **MCP Protocol Handler** - Integrates with protocol_transport_core
pub struct McpProtocolHandler {
    /// Server capabilities (for server mode)
    capabilities: Option<ServerCapabilities>,
    /// Authentication handler
    auth_handler: Option<Box<dyn AuthHandler>>,
    /// Tool provider (pure abstraction)
    tool_provider: Option<Box<dyn ToolProvider>>,
    /// Query mode: single provider or aggregate all providers
    query_mode: QueryMode,
}

/// **Tool Provider Trait** - Pure abstraction for tool operations
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// List tools from this provider
    fn list_tools(&self) -> Result<Vec<Tool>, ProtocolError>;

    /// Execute tool via this provider
    async fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError>;
}

/// **Query Mode** - How to handle tool listing
#[derive(Debug, Clone)]
pub enum QueryMode {
    /// List tools from single provider (standard MCP behavior)
    Single,
    /// Aggregate tools from all providers (special case)
    Aggregate,
}

impl Default for QueryMode {
    fn default() -> Self {
        QueryMode::Single
    }
}

impl McpProtocolHandler {
    /// Create new MCP protocol handler
    pub fn new() -> Self {
        Self {
            capabilities: None,
            auth_handler: None,
            tool_provider: None,
            query_mode: QueryMode::Single,
        }
    }

    /// Configure server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = Some(capabilities);
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

    /// Configure query mode
    pub fn with_query_mode(mut self, query_mode: QueryMode) -> Self {
        self.query_mode = query_mode;
        self
    }

    /// Handle MCP method request
    async fn handle_mcp_method(
        &self,
        method: &str,
        params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, ProtocolError> {
        match method {
            "initialize" => self.handle_initialize(params, id),
            "tools/list" => self.handle_list_tools(params, id),
            "tools/call" => self.handle_call_tool(params, id).await,
            _ => Ok(JsonRpcResponse::error(
                id,
                JsonRpcError::method_not_found(&format!("Method '{}' not found", method)),
            )),
        }
    }

    fn handle_initialize(
        &self,
        params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, ProtocolError> {
        let _init_request: InitializeRequest = serde_json::from_value(params)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid initialize request: {}", e)))?;

        let result = InitializeResult {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: self.capabilities.clone().unwrap_or_default(),
            server_info: ServerInfo {
                name: "promptfleet-mcp-server".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("PromptFleet MCP Server".to_string()),
            },
        };

        Ok(JsonRpcResponse::success(id, serde_json::to_value(result)?))
    }

    fn handle_list_tools(
        &self,
        _params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, ProtocolError> {
        let tools = match &self.tool_provider {
            Some(provider) => provider.list_tools()?,
            None => vec![], // No provider configured
        };

        let result = ListToolsResult { tools };
        Ok(JsonRpcResponse::success(id, serde_json::to_value(result)?))
    }

    async fn handle_call_tool(
        &self,
        params: serde_json::Value,
        id: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, ProtocolError> {
        let call_request: CallToolRequest = serde_json::from_value(params)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid call_tool request: {}", e)))?;

        let result = match &self.tool_provider {
            Some(provider) => provider.call_tool(&call_request.name, call_request.arguments).await?,
            None => CallToolResult {
                content: vec![Content::text("No tool provider configured")],
                is_error: Some(true),
            },
        };

        Ok(JsonRpcResponse::success(id, serde_json::to_value(result)?))
    }
}

impl ProtocolHandler for McpProtocolHandler {
    type Request = JsonRpcRequest;
    type Response = JsonRpcResponse;
    type Error = ProtocolError;

    fn protocol_name(&self) -> &'static str {
        "MCP"
    }

    fn encode_request(&self, request: &Self::Request) -> Result<UniversalRequest, Self::Error> {
        let body = serde_json::to_vec(request)?;
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert(
            "accept".to_string(),
            "application/json, text/event-stream".to_string(),
        );
        headers.insert("x-protocol".to_string(), "MCP".to_string());

        if let Some(id) = &request.id {
            headers.insert("x-correlation-id".to_string(), id.to_string());
        }

        Ok(UniversalRequest {
            method: "POST".to_string(),
            uri: "/mcp/rpc".to_string(),
            headers,
            body,
            protocol: "MCP".to_string(),
            correlation_id: request
                .id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        })
    }

    fn decode_request(&self, universal: &UniversalRequest) -> Result<Self::Request, Self::Error> {
        let request: JsonRpcRequest = serde_json::from_slice(&universal.body)?;
        Ok(request)
    }

    fn encode_response(&self, response: &Self::Response) -> Result<UniversalResponse, Self::Error> {
        let body = serde_json::to_vec(response)?;
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("x-protocol".to_string(), "MCP".to_string());

        Ok(UniversalResponse {
            status: 200,
            headers,
            body,
            protocol: "MCP".to_string(),
            correlation_id: response
                .id
                .as_ref()
                .map(|id| id.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        })
    }

    fn decode_response(
        &self,
        universal: &UniversalResponse,
    ) -> Result<Self::Response, Self::Error> {
        let response: JsonRpcResponse = serde_json::from_slice(&universal.body)?;
        Ok(response)
    }
}

impl AsyncProtocolHandler for McpProtocolHandler {
    fn protocol_name(&self) -> &'static str {
        "MCP"
    }

    fn handle_request_sync(
        &self,
        request: UniversalRequest,
    ) -> Result<UniversalResponse, ProtocolError> {
        // Parse JSON-RPC from request body
        let body_str = String::from_utf8(request.body)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid UTF-8 in request body: {}", e)))?;

        let json_request: serde_json::Value = serde_json::from_str(&body_str)
            .map_err(|e| ProtocolError::Parsing(format!("Invalid JSON in request body: {}", e)))?;

        // Extract JSON-RPC fields
        let method = json_request["method"]
            .as_str()
            .ok_or_else(|| ProtocolError::Parsing("Missing 'method' field".to_string()))?;
        let params = json_request.get("params").cloned().unwrap_or_default();
        let id = json_request.get("id").cloned();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let response = tokio::runtime::Handle::current().block_on(
                self.handle_mcp_method(method, params, id)
            ).map_err(|e| ProtocolError::internal_error(&format!("MCP error: {:?}", e)))?;

            let response_body =
                serde_json::to_string(&response).map_err(ProtocolError::Serialization)?;

            Ok(UniversalResponse {
                status: 200,
                headers: [("content-type".to_string(), "application/json".to_string())]
                    .iter()
                    .cloned()
                    .collect(),
                body: response_body.into_bytes(),
                protocol: "MCP".to_string(),
                correlation_id: request.correlation_id,
            })
        }

        #[cfg(target_arch = "wasm32")]
        Err(ProtocolError::internal_error(
            "Sync MCP handler not supported in WASM; use async handler",
        ))
    }
}

impl Default for McpProtocolHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// **Quick setup function** for MCP protocol handler
pub fn create_mcp_handler() -> McpProtocolHandler {
    McpProtocolHandler::new().with_capabilities(ServerCapabilities::default())
}

/// **Quick setup function** for MCP protocol handler with custom capabilities
pub fn create_mcp_handler_with_capabilities(
    capabilities: ServerCapabilities,
) -> McpProtocolHandler {
    McpProtocolHandler::new().with_capabilities(capabilities)
}
