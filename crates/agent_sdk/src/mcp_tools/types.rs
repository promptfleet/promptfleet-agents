//! Unified MCP types that abstract over rmcp (native) and mcp_protocol (WASM).

use crate::mcp_tools::error::McpToolError;
use std::collections::HashMap;

pub const MCP_FORWARDED_HEADERS_META_KEY: &str = "forwarded_headers";
pub use protocol_transport_core::MCP_CALLER_AUTHORIZATION_HEADER;

pub fn build_forwarded_headers_meta(
    request_headers: Option<&HashMap<String, String>>,
    include_authorization: bool,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let Some(request_headers) = request_headers else {
        return None;
    };

    let delegated_authorization = request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(MCP_CALLER_AUTHORIZATION_HEADER))
        .map(|(_, value)| value.clone());
    let mut forwarded = protocol_transport_core::sanitize_headers(request_headers).into_map();
    forwarded.retain(|name, _| !name.eq_ignore_ascii_case("mcp-session-id"));
    forwarded.retain(|name, _| !name.eq_ignore_ascii_case(MCP_CALLER_AUTHORIZATION_HEADER));
    forwarded.retain(|name, _| !name.eq_ignore_ascii_case("authorization"));
    if include_authorization && let Some(authorization) = delegated_authorization {
        forwarded.insert("authorization".to_string(), authorization);
    }
    if forwarded.is_empty() {
        return None;
    }

    let forwarded_headers = forwarded
        .into_iter()
        .map(|(key, value)| (key, serde_json::Value::String(value)))
        .collect::<serde_json::Map<_, _>>();

    let mut meta = serde_json::Map::new();
    meta.insert(
        MCP_FORWARDED_HEADERS_META_KEY.to_string(),
        serde_json::Value::Object(forwarded_headers),
    );
    Some(meta)
}

pub fn merge_request_meta(
    request_meta: &serde_json::Map<String, serde_json::Value>,
    dynamic_meta: Option<serde_json::Map<String, serde_json::Value>>,
) -> Option<serde_json::Value> {
    let mut merged = request_meta.clone();
    if let Some(dynamic_meta) = dynamic_meta {
        for (key, value) in dynamic_meta {
            merged.insert(key, value);
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(merged))
    }
}

/// Tool descriptor from an MCP server (backend-agnostic).
#[derive(Debug, Clone)]
pub struct McpToolDescriptor {
    /// Which MCP server this tool belongs to
    pub server_id: String,
    /// Tool name as reported by the MCP server
    pub name: String,
    /// Human-readable description
    pub description: Option<String>,
    /// JSON Schema for the tool's input parameters
    pub input_schema: serde_json::Value,
}

/// Result of calling an MCP tool (backend-agnostic).
#[derive(Debug, Clone)]
pub struct McpCallResult {
    /// Content items returned by the tool
    pub content: Vec<McpContent>,
    /// Whether the tool reported an error
    pub is_error: bool,
}

/// Content item from an MCP tool result.
#[derive(Debug, Clone)]
pub enum McpContent {
    /// Text content
    Text(String),
    // Future: Image, Resource, Embedded, etc.
}

impl McpCallResult {
    /// Convert to a JSON value suitable for returning from a ToolSpec executor.
    pub fn to_json(&self) -> serde_json::Value {
        if self.is_error {
            let error_text = self
                .content
                .iter()
                .filter_map(|c| match c {
                    McpContent::Text(t) => Some(t.as_str()),
                })
                .collect::<Vec<_>>()
                .join("\n");
            return serde_json::json!({ "error": error_text });
        }

        if self.content.len() == 1 {
            match &self.content[0] {
                McpContent::Text(t) => {
                    // Try parsing as JSON (many MCP tools return JSON strings)
                    serde_json::from_str(t).unwrap_or_else(|_| serde_json::json!({ "text": t }))
                }
            }
        } else {
            let parts: Vec<serde_json::Value> = self
                .content
                .iter()
                .map(|c| match c {
                    McpContent::Text(t) => serde_json::json!({ "type": "text", "text": t }),
                })
                .collect();
            serde_json::json!({ "content": parts })
        }
    }
}

/// Abstraction over MCP client backends (rmcp on native, mcp_protocol on WASM).
///
/// Consumer code is target-agnostic — it works with [`McpToolDescriptor`] and
/// [`McpCallResult`] regardless of which backend handles the protocol.
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait McpToolSource: Send + Sync {
    /// List all tools from a connected MCP server.
    async fn list_tools(&self, server_id: &str) -> Result<Vec<McpToolDescriptor>, McpToolError>;

    /// Call a tool on a specific server.
    async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<McpCallResult, McpToolError>;

    /// Call a tool with request-scoped forwarded headers when the backend
    /// supports per-request propagation.
    async fn call_tool_with_headers(
        &self,
        server_id: &str,
        tool_name: &str,
        args: serde_json::Value,
        request_headers: Option<&HashMap<String, String>>,
    ) -> Result<McpCallResult, McpToolError> {
        let _ = request_headers;
        self.call_tool(server_id, tool_name, args).await
    }

    /// Health check a server connection.
    async fn health_check(&self, server_id: &str) -> Result<bool, McpToolError>;

    /// List all connected server IDs.
    fn server_ids(&self) -> Vec<String>;

    /// List all tools from all connected servers.
    async fn list_all_tools(&self) -> Result<Vec<McpToolDescriptor>, McpToolError> {
        let mut all = Vec::new();
        for sid in self.server_ids() {
            match self.list_tools(&sid).await {
                Ok(tools) => all.extend(tools),
                Err(e) => log::warn!("Failed to list tools from MCP server {}: {}", sid, e),
            }
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MCP_CALLER_AUTHORIZATION_HEADER, MCP_FORWARDED_HEADERS_META_KEY,
        build_forwarded_headers_meta, merge_request_meta,
    };
    use std::collections::HashMap;

    #[test]
    fn build_forwarded_headers_meta_strips_transport_headers() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer user-token".to_string());
        headers.insert("traceparent".to_string(), "00-abc-123-01".to_string());
        headers.insert("mcp-session-id".to_string(), "session-1".to_string());
        headers.insert("content-length".to_string(), "10".to_string());

        let meta = build_forwarded_headers_meta(Some(&headers), false).expect("meta");
        let forwarded = meta
            .get(MCP_FORWARDED_HEADERS_META_KEY)
            .and_then(|value| value.as_object())
            .expect("forwarded headers object");

        assert_eq!(
            forwarded
                .get("traceparent")
                .and_then(|value| value.as_str()),
            Some("00-abc-123-01")
        );
        assert!(!forwarded.contains_key("authorization"));
        assert!(!forwarded.contains_key("mcp-session-id"));
        assert!(!forwarded.contains_key("content-length"));
    }

    #[test]
    fn test_delegated_authorization_replaces_runtime_authorization_when_enabled() {
        let headers = HashMap::from([
            (
                "authorization".to_string(),
                "Bearer runtime-token".to_string(),
            ),
            (
                MCP_CALLER_AUTHORIZATION_HEADER.to_string(),
                "Bearer delegated-token".to_string(),
            ),
        ]);

        let meta = build_forwarded_headers_meta(Some(&headers), true).expect("meta");
        let forwarded = meta[MCP_FORWARDED_HEADERS_META_KEY]
            .as_object()
            .expect("forwarded headers object");

        assert_eq!(
            forwarded.get("authorization").and_then(|value| value.as_str()),
            Some("Bearer delegated-token")
        );
        assert!(!forwarded.contains_key(MCP_CALLER_AUTHORIZATION_HEADER));
    }

    #[test]
    fn test_delegated_authorization_is_removed_when_forwarding_is_disabled() {
        let headers = HashMap::from([(
            MCP_CALLER_AUTHORIZATION_HEADER.to_string(),
            "Bearer delegated-token".to_string(),
        )]);

        assert!(build_forwarded_headers_meta(Some(&headers), false).is_none());
    }

    #[test]
    fn test_runtime_authorization_is_not_forwarded_without_delegation_header() {
        let headers = HashMap::from([(
            "authorization".to_string(),
            "Bearer runtime-token".to_string(),
        )]);

        assert!(build_forwarded_headers_meta(Some(&headers), true).is_none());
    }

    #[test]
    fn merge_request_meta_keeps_static_and_dynamic_values() {
        let mut request_meta = serde_json::Map::new();
        request_meta.insert("static".to_string(), serde_json::json!("value"));
        let mut dynamic_meta = serde_json::Map::new();
        dynamic_meta.insert("dynamic".to_string(), serde_json::json!(true));

        let merged = merge_request_meta(&request_meta, Some(dynamic_meta)).unwrap();

        assert_eq!(merged["static"], "value");
        assert_eq!(merged["dynamic"], true);
    }
}
