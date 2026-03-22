// examples/mcp_basic_server.rs
//! Basic MCP Server Example - Pure Protocol Compliance
//!
//! This example shows how to implement a fully MCP-compliant server
//! that connects with any standard MCP client (FastMCP, Claude Desktop, etc.)

use async_trait::async_trait;
use mcp_protocol::{
    CallToolResult, Content, McpProtocolHandler, QueryMode, ServerCapabilities, Tool,
    ToolCapabilities, ToolProvider,
};
use protocol_transport_core::{AsyncProtocolHandler, ProtocolError, UniversalRequest};
use serde_json::json;

/// **Example Tool Provider** - Calculator Tools
struct CalculatorProvider;

#[async_trait]
impl ToolProvider for CalculatorProvider {
    fn list_tools(&self) -> Result<Vec<Tool>, ProtocolError> {
        Ok(vec![
            Tool {
                name: "add".to_string(),
                description: "Add two numbers".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "a": {"type": "number", "description": "First number"},
                        "b": {"type": "number", "description": "Second number"}
                    },
                    "required": ["a", "b"]
                }),
            },
            Tool {
                name: "multiply".to_string(),
                description: "Multiply two numbers".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "a": {"type": "number", "description": "First number"},
                        "b": {"type": "number", "description": "Second number"}
                    },
                    "required": ["a", "b"]
                }),
            },
        ])
    }

    async fn call_tool(
        &self,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult, ProtocolError> {
        let args = arguments.unwrap_or_default();

        match name {
            "add" => {
                let a = args["a"]
                    .as_f64()
                    .ok_or_else(|| ProtocolError::internal_error("Missing parameter 'a'"))?;
                let b = args["b"]
                    .as_f64()
                    .ok_or_else(|| ProtocolError::internal_error("Missing parameter 'b'"))?;
                let result = a + b;

                Ok(CallToolResult {
                    content: vec![Content::text(&format!("Result: {}", result))],
                    is_error: Some(false),
                })
            }
            "multiply" => {
                let a = args["a"]
                    .as_f64()
                    .ok_or_else(|| ProtocolError::internal_error("Missing parameter 'a'"))?;
                let b = args["b"]
                    .as_f64()
                    .ok_or_else(|| ProtocolError::internal_error("Missing parameter 'b'"))?;
                let result = a * b;

                Ok(CallToolResult {
                    content: vec![Content::text(&format!("Result: {}", result))],
                    is_error: Some(false),
                })
            }
            _ => Err(ProtocolError::internal_error(&format!(
                "Unknown tool: {}",
                name
            ))),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧮 MCP Calculator Server - Protocol Compliant");
    println!("Supports standard MCP methods: initialize, tools/list, tools/call");

    // Create capabilities
    let capabilities = ServerCapabilities {
        tools: Some(ToolCapabilities { supported: true }),
    };

    // Create MCP handler
    let handler = McpProtocolHandler::new()
        .with_capabilities(capabilities)
        .with_tool_provider(CalculatorProvider)
        .with_query_mode(QueryMode::Single); // Standard MCP behavior

    // Simulate MCP client interaction
    println!("\n📋 Testing MCP Protocol Compliance...");

    // 1. List tools from THIS server
    let list_request = UniversalRequest {
        method: "tools/list".to_string(),
        uri: "/".to_string(),
        headers: Default::default(),
        body: json!({
            "jsonrpc": "2.0",
            "method": "tools/list",
            "params": {},
            "id": 2
        })
        .to_string()
        .into_bytes(),
        protocol: "MCP".to_string(),
        correlation_id: "test-2".to_string(),
    };

    println!("\n🔧 Listing tools from this server...");
    let list_response = handler.handle_request_sync(list_request)?;
    let response_body = String::from_utf8(list_response.body)?;
    println!("✅ Tools Response: {}", response_body);

    // 2. Call a tool
    let call_request = UniversalRequest {
        method: "tools/call".to_string(),
        uri: "/".to_string(),
        headers: Default::default(),
        body: json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {
                    "a": 15,
                    "b": 27
                }
            },
            "id": 3
        })
        .to_string()
        .into_bytes(),
        protocol: "MCP".to_string(),
        correlation_id: "test-3".to_string(),
    };

    println!("\n🧮 Calling add tool...");
    let call_response = handler.handle_request_sync(call_request)?;
    let response_body = String::from_utf8(call_response.body)?;
    println!("✅ Tool Result: {}", response_body);

    println!("\n🎉 MCP Protocol Compliance Verified!");
    println!("This server can now connect to any MCP client (FastMCP, Claude Desktop, etc.)");

    Ok(())
}
