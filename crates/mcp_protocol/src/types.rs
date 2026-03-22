//! # MCP Protocol Types
//!
//! **JSON-RPC 2.0 and MCP specification compliant** data structures
//! - Fully compatible with FastMCP and other MCP implementations
//! - Standard OAuth 2.1 Bearer token authentication support
//! - Comprehensive tool, memory, and capability definitions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// **JSON-RPC 2.0 Request** - Standard wire format for MCP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID (optional for notifications)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    /// Method name (e.g., "tools/list", "tools/call")
    pub method: String,
    /// Method parameters (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// **JSON-RPC 2.0 Response** - Standard response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Response ID (matches request ID)
    pub id: Option<serde_json::Value>,
    /// Success result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error information (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// **JSON-RPC 2.0 Error** - Standard error format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code (standard JSON-RPC error codes)
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create new JSON-RPC request
    pub fn new(
        method: &str,
        params: Option<serde_json::Value>,
        id: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id,
        }
    }

    /// Create notification (no response expected)
    pub fn notification(method: &str, params: Option<serde_json::Value>) -> Self {
        Self::new(method, params, None)
    }

    /// Create request with string ID
    pub fn with_id(method: &str, params: Option<serde_json::Value>, id: &str) -> Self {
        Self::new(
            method,
            params,
            Some(serde_json::Value::String(id.to_string())),
        )
    }
}

impl JsonRpcResponse {
    /// Create success response
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create error response
    pub fn error(id: Option<serde_json::Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

impl JsonRpcError {
    /// Method not found (-32601)
    pub fn method_not_found(message: &str) -> Self {
        Self {
            code: -32601,
            message: message.to_string(),
            data: None,
        }
    }

    /// Invalid params (-32602)
    pub fn invalid_params(message: &str) -> Self {
        Self {
            code: -32602,
            message: message.to_string(),
            data: None,
        }
    }

    /// Internal error (-32603)
    pub fn internal_error(message: &str) -> Self {
        Self {
            code: -32603,
            message: message.to_string(),
            data: None,
        }
    }
}

// === MCP Protocol-Specific Types ===

/// **Initialize Request** - MCP handshake
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    /// Protocol version requested by client
    #[serde(alias = "protocol_version")]
    pub protocol_version: String,
    /// Client capabilities
    pub capabilities: ClientCapabilities,
    /// Client information
    #[serde(alias = "client_info")]
    pub client_info: ClientInfo,
}

/// **Initialize Result** - MCP handshake response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Protocol version supported by server
    #[serde(alias = "protocol_version")]
    pub protocol_version: String,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
    /// Server information
    #[serde(alias = "server_info")]
    pub server_info: ServerInfo,
}

/// **Client Information**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// Client name
    pub name: String,
    /// Client version
    pub version: String,
    /// Client description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// **Server Information**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
    /// Server description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// **Client Capabilities**
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    /// Tool execution support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolCapabilities>,
}

/// **Server Capabilities**
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    /// Tool execution support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolCapabilities>,
}

/// **Tool Capabilities**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapabilities {
    /// Tool listing support
    pub supported: bool,
}

// === Tool Operations ===

/// **List Tools Result**
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    /// Available tools
    pub tools: Vec<Tool>,
}

/// **Tool Definition**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// JSON Schema for tool parameters
    #[serde(alias = "input_schema")]
    pub input_schema: serde_json::Value,
}

/// **Call Tool Request**
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    /// Tool name to execute
    pub name: String,
    /// Tool arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
    /// Optional MCP request metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// **Call Tool Result**
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    /// Tool execution result content
    pub content: Vec<Content>,
    /// Whether execution resulted in error
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "is_error")]
    pub is_error: Option<bool>,
}

/// **Content** - Multi-modal content representation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text {
        /// Text content
        text: String,
    },
}

/// **Bearer Token Authentication**
#[derive(Debug, Clone)]
pub struct BearerToken {
    /// OAuth 2.1 access token
    pub access_token: String,
    /// Token type (always "Bearer")
    pub token_type: String,
}

impl BearerToken {
    /// Create new bearer token
    pub fn new(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_string(),
            token_type: "Bearer".to_string(),
        }
    }

    /// Format as Authorization header value
    pub fn to_authorization_header(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }
}

impl Tool {
    /// Create simple tool with string parameters
    pub fn simple(name: &str, description: &str, parameters: &[&str]) -> Self {
        let mut properties = serde_json::Map::new();
        for param in parameters {
            properties.insert(
                param.to_string(),
                serde_json::json!({
                    "type": "string",
                    "description": format!("Parameter: {}", param)
                }),
            );
        }

        let schema = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": parameters
        });

        Self {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: schema,
        }
    }
}

impl Content {
    /// Create text content
    pub fn text(text: &str) -> Self {
        Content::Text {
            text: text.to_string(),
        }
    }
}
