//! # Protocol Transport Core
//!
//! **Universal transport foundation** for multiple protocols
//! - A2A (Agent-to-Agent)
//! - MCP (Model Context Protocol)  
//! - Redpanda PubSub
//! - REST (GET/PUT/POST)
//!
//! ## Design Principles
//! - **Protocol Agnostic**: Transport independent of protocol details
//! - **WASM-Optimized**: SpinKube-first design
//! - **Zero-Copy**: Minimal serialization overhead
//! - **Composable**: Mix and match transports and protocols

pub mod error;
pub mod forward_headers;
pub mod headers;
pub mod jsonrpc;
pub mod serialization;
#[cfg(feature = "server")]
pub mod server;
pub mod streaming;
#[cfg(feature = "client")]
pub mod transport;

#[cfg(feature = "client")]
pub use transport::*;

// Re-export core types for convenience
pub use error::{ProtocolError, ProtocolResult, TransportError, TransportResult};
pub use forward_headers::{
    ForwardedHeaders, MCP_CALLER_AUTHORIZATION_HEADER, sanitize_header_map, sanitize_headers,
};
pub use headers::ProtocolHeaders;
pub use jsonrpc::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcId, JsonRpcIncoming, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, error_codes,
};
#[cfg(not(target_arch = "wasm32"))]
pub use streaming::IdleTimeoutStream;
pub use streaming::{RPC_REQUEST_TIMEOUT, StreamingPolicy};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// **Universal Request** - Protocol-agnostic request container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalRequest {
    pub method: String,
    pub uri: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub protocol: String,
    pub correlation_id: String,
}

/// **Universal Response** - Protocol-agnostic response container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniversalResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub protocol: String,
    pub correlation_id: String,
}

/// **Protocol Handler Trait** - Implement for each protocol
pub trait ProtocolHandler {
    type Request;
    type Response;
    type Error;

    /// Protocol name (e.g., "A2A", "MCP", "REST")
    fn protocol_name(&self) -> &'static str;

    /// Serialize protocol request to universal format
    fn encode_request(&self, request: &Self::Request) -> Result<UniversalRequest, Self::Error>;

    /// Deserialize universal request to protocol format
    fn decode_request(&self, universal: &UniversalRequest) -> Result<Self::Request, Self::Error>;

    /// Serialize protocol response to universal format
    fn encode_response(&self, response: &Self::Response) -> Result<UniversalResponse, Self::Error>;

    /// Deserialize universal response to protocol format
    fn decode_response(&self, universal: &UniversalResponse)
    -> Result<Self::Response, Self::Error>;
}

/// **Transport Trait** - HTTP, SSE, etc.
///
/// Uses `async fn` in trait without `Send` bounds because WASM
/// targets (Spin SDK) do not produce `Send` futures.
#[allow(async_fn_in_trait)]
pub trait Transport {
    async fn send(&self, request: UniversalRequest) -> Result<UniversalResponse, TransportError>;
    async fn health_check(&self) -> Result<(), TransportError>;
}

/// **Server Router** - Route requests to appropriate protocol handlers (Spin async compatible)
pub struct ProtocolRouter {
    handlers: HashMap<String, ProtocolHandlerFn>,
}

/// **Protocol Handler Function** - Function-based approach for WASM compatibility
pub type ProtocolHandlerFn =
    Box<dyn Fn(UniversalRequest) -> Result<UniversalResponse, ProtocolError> + Send + Sync>;

/// **Async Protocol Handler** - Spin SDK async compatible
pub trait AsyncProtocolHandler: Send + Sync {
    /// Protocol name (e.g., "A2A", "MCP", "REST")
    fn protocol_name(&self) -> &'static str;

    /// Handle request synchronously (async operations handled internally)
    fn handle_request_sync(
        &self,
        request: UniversalRequest,
    ) -> Result<UniversalResponse, ProtocolError>;
}

impl ProtocolRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register protocol handler function
    pub fn register_fn<F>(&mut self, protocol: &str, handler: F)
    where
        F: Fn(UniversalRequest) -> Result<UniversalResponse, ProtocolError> + Send + Sync + 'static,
    {
        self.handlers
            .insert(protocol.to_string(), Box::new(handler));
    }

    /// Register async protocol handler
    pub fn register<H>(&mut self, protocol: &str, handler: H)
    where
        H: AsyncProtocolHandler + 'static,
    {
        let handler_fn =
            move |request: UniversalRequest| -> Result<UniversalResponse, ProtocolError> {
                handler.handle_request_sync(request)
            };
        self.handlers
            .insert(protocol.to_string(), Box::new(handler_fn));
    }

    /// Route request to appropriate handler
    pub fn route_request(
        &self,
        request: UniversalRequest,
    ) -> Result<UniversalResponse, ProtocolError> {
        let handler = self
            .handlers
            .get(&request.protocol)
            .ok_or_else(|| ProtocolError::ProtocolNotFound(request.protocol.clone()))?;

        handler(request)
    }

    /// List available protocols
    pub fn list_protocols(&self) -> Vec<String> {
        self.handlers.keys().cloned().collect()
    }
}

impl Default for ProtocolRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Mock protocol handler for testing
    struct MockProtocolHandler {
        protocol_name: &'static str,
        should_error: bool,
    }

    impl MockProtocolHandler {
        fn new(protocol_name: &'static str) -> Self {
            Self {
                protocol_name,
                should_error: false,
            }
        }

        fn new_with_error(protocol_name: &'static str) -> Self {
            Self {
                protocol_name,
                should_error: true,
            }
        }
    }

    impl AsyncProtocolHandler for MockProtocolHandler {
        fn protocol_name(&self) -> &'static str {
            self.protocol_name
        }

        fn handle_request_sync(
            &self,
            request: UniversalRequest,
        ) -> Result<UniversalResponse, ProtocolError> {
            if self.should_error {
                return Err(ProtocolError::Internal("Handler error".to_string()));
            }

            Ok(UniversalResponse {
                status: 200,
                headers: HashMap::new(),
                body: format!("Response from {}", self.protocol_name).into_bytes(),
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            })
        }
    }

    fn create_test_universal_request(protocol: &str) -> UniversalRequest {
        UniversalRequest {
            method: "POST".to_string(),
            uri: "/test".to_string(),
            headers: HashMap::new(),
            body: b"test request".to_vec(),
            protocol: protocol.to_string(),
            correlation_id: "test-correlation-123".to_string(),
        }
    }

    #[test]
    fn test_universal_request_creation() {
        let request = create_test_universal_request("A2A");

        assert_eq!(request.method, "POST");
        assert_eq!(request.uri, "/test");
        assert!(request.headers.is_empty());
        assert_eq!(request.body, b"test request");
        assert_eq!(request.protocol, "A2A");
        assert_eq!(request.correlation_id, "test-correlation-123");
    }

    #[test]
    fn test_universal_request_debug_format() {
        let request = create_test_universal_request("DEBUG");
        let debug_str = format!("{:?}", request);

        assert!(debug_str.contains("POST"));
        assert!(debug_str.contains("/test"));
        assert!(debug_str.contains("DEBUG"));
        assert!(debug_str.contains("test-correlation-123"));
    }

    #[test]
    fn test_universal_request_clone() {
        let original = create_test_universal_request("CLONE");
        let cloned = original.clone();

        assert_eq!(original.method, cloned.method);
        assert_eq!(original.uri, cloned.uri);
        assert_eq!(original.protocol, cloned.protocol);
        assert_eq!(original.correlation_id, cloned.correlation_id);
        assert_eq!(original.body, cloned.body);
    }

    #[test]
    fn test_universal_request_serialization() {
        let request = create_test_universal_request("SERIALIZE");

        // Test serialization
        let serialized = serde_json::to_string(&request).unwrap();
        assert!(serialized.contains("POST"));
        assert!(serialized.contains("/test"));
        assert!(serialized.contains("SERIALIZE"));

        // Test deserialization
        let deserialized: UniversalRequest = serde_json::from_str(&serialized).unwrap();
        assert_eq!(request.method, deserialized.method);
        assert_eq!(request.uri, deserialized.uri);
        assert_eq!(request.protocol, deserialized.protocol);
    }

    #[test]
    fn test_universal_response_creation() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let response = UniversalResponse {
            status: 201,
            headers: headers.clone(),
            body: b"test response".to_vec(),
            protocol: "TEST".to_string(),
            correlation_id: "response-correlation".to_string(),
        };

        assert_eq!(response.status, 201);
        assert_eq!(
            response.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(response.body, b"test response");
        assert_eq!(response.protocol, "TEST");
        assert_eq!(response.correlation_id, "response-correlation");
    }

    #[test]
    fn test_universal_response_debug_format() {
        let response = UniversalResponse {
            status: 404,
            headers: HashMap::new(),
            body: b"not found".to_vec(),
            protocol: "DEBUG".to_string(),
            correlation_id: "debug-correlation".to_string(),
        };

        let debug_str = format!("{:?}", response);
        assert!(debug_str.contains("404"));
        assert!(debug_str.contains("DEBUG"));
        assert!(debug_str.contains("debug-correlation"));
    }

    #[test]
    fn test_universal_response_clone() {
        let mut headers = HashMap::new();
        headers.insert("test-header".to_string(), "test-value".to_string());

        let original = UniversalResponse {
            status: 500,
            headers: headers.clone(),
            body: b"clone test".to_vec(),
            protocol: "CLONE".to_string(),
            correlation_id: "clone-correlation".to_string(),
        };

        let cloned = original.clone();
        assert_eq!(original.status, cloned.status);
        assert_eq!(original.headers, cloned.headers);
        assert_eq!(original.body, cloned.body);
        assert_eq!(original.protocol, cloned.protocol);
        assert_eq!(original.correlation_id, cloned.correlation_id);
    }

    #[test]
    fn test_universal_response_serialization() {
        let response = UniversalResponse {
            status: 200,
            headers: HashMap::new(),
            body: b"serialize test".to_vec(),
            protocol: "SERIALIZE".to_string(),
            correlation_id: "serialize-correlation".to_string(),
        };

        // Test serialization
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(serialized.contains("200"));
        assert!(serialized.contains("SERIALIZE"));

        // Test deserialization
        let deserialized: UniversalResponse = serde_json::from_str(&serialized).unwrap();
        assert_eq!(response.status, deserialized.status);
        assert_eq!(response.protocol, deserialized.protocol);
        assert_eq!(response.correlation_id, deserialized.correlation_id);
    }

    #[test]
    fn test_protocol_router_new() {
        let router = ProtocolRouter::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_protocol_router_default() {
        let router = ProtocolRouter::default();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_protocol_router_register_fn() {
        let mut router = ProtocolRouter::new();

        let handler_fn = |request: UniversalRequest| -> Result<UniversalResponse, ProtocolError> {
            Ok(UniversalResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"function handler response".to_vec(),
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            })
        };

        router.register_fn("FUNCTION", handler_fn);
        assert_eq!(router.handlers.len(), 1);
        assert!(router.handlers.contains_key("FUNCTION"));
    }

    #[test]
    fn test_protocol_router_register_async_handler() {
        let mut router = ProtocolRouter::new();
        let handler = MockProtocolHandler::new("ASYNC");

        router.register("ASYNC", handler);
        assert_eq!(router.handlers.len(), 1);
        assert!(router.handlers.contains_key("ASYNC"));
    }

    #[test]
    fn test_protocol_router_route_request_success() {
        let mut router = ProtocolRouter::new();
        router.register("SUCCESS", MockProtocolHandler::new("SUCCESS"));

        let request = create_test_universal_request("SUCCESS");
        let result = router.route_request(request);

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.protocol, "SUCCESS");
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "Response from SUCCESS"
        );
    }

    #[test]
    fn test_protocol_router_route_request_protocol_not_found() {
        let router = ProtocolRouter::new();
        let request = create_test_universal_request("UNKNOWN");

        let result = router.route_request(request);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProtocolError::ProtocolNotFound(protocol) => {
                assert_eq!(protocol, "UNKNOWN");
            }
            _ => panic!("Expected ProtocolNotFound error"),
        }
    }

    #[test]
    fn test_protocol_router_route_request_handler_error() {
        let mut router = ProtocolRouter::new();
        router.register("ERROR", MockProtocolHandler::new_with_error("ERROR"));

        let request = create_test_universal_request("ERROR");
        let result = router.route_request(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            ProtocolError::Internal(msg) => {
                assert_eq!(msg, "Handler error");
            }
            _ => panic!("Expected Internal error"),
        }
    }

    #[test]
    fn test_protocol_router_list_protocols() {
        let mut router = ProtocolRouter::new();

        // Empty router
        assert!(router.list_protocols().is_empty());

        // Add protocols
        router.register("A2A", MockProtocolHandler::new("A2A"));
        router.register("MCP", MockProtocolHandler::new("MCP"));
        router.register("REST", MockProtocolHandler::new("REST"));

        let protocols = router.list_protocols();
        assert_eq!(protocols.len(), 3);
        assert!(protocols.contains(&"A2A".to_string()));
        assert!(protocols.contains(&"MCP".to_string()));
        assert!(protocols.contains(&"REST".to_string()));
    }

    #[test]
    fn test_protocol_router_multiple_handlers() {
        let mut router = ProtocolRouter::new();

        router.register("A2A", MockProtocolHandler::new("A2A"));
        router.register("MCP", MockProtocolHandler::new("MCP"));

        // Test A2A request
        let a2a_request = create_test_universal_request("A2A");
        let a2a_result = router.route_request(a2a_request).unwrap();
        assert_eq!(a2a_result.protocol, "A2A");
        assert!(String::from_utf8(a2a_result.body).unwrap().contains("A2A"));

        // Test MCP request
        let mcp_request = create_test_universal_request("MCP");
        let mcp_result = router.route_request(mcp_request).unwrap();
        assert_eq!(mcp_result.protocol, "MCP");
        assert!(String::from_utf8(mcp_result.body).unwrap().contains("MCP"));
    }

    #[test]
    fn test_protocol_router_overwrite_handler() {
        let mut router = ProtocolRouter::new();

        // Register initial handler
        router.register("OVERWRITE", MockProtocolHandler::new("INITIAL"));
        assert_eq!(router.handlers.len(), 1);

        // Overwrite with new handler
        router.register("OVERWRITE", MockProtocolHandler::new("OVERWRITTEN"));
        assert_eq!(router.handlers.len(), 1); // Should still be 1

        // Test that new handler is used
        let request = create_test_universal_request("OVERWRITE");
        let result = router.route_request(request).unwrap();
        assert!(
            String::from_utf8(result.body)
                .unwrap()
                .contains("OVERWRITTEN")
        );
    }

    #[test]
    fn test_protocol_handler_fn_type() {
        // Test that we can create a protocol handler function directly
        let handler_fn: ProtocolHandlerFn = Box::new(|request: UniversalRequest| {
            Ok(UniversalResponse {
                status: 201,
                headers: HashMap::new(),
                body: b"direct function".to_vec(),
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            })
        });

        let request = create_test_universal_request("DIRECT");
        let result = handler_fn(request);

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status, 201);
        assert_eq!(String::from_utf8(response.body).unwrap(), "direct function");
    }

    #[test]
    fn test_universal_request_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_string(), "Bearer token123".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());

        let request = UniversalRequest {
            method: "PUT".to_string(),
            uri: "/api/v1/test".to_string(),
            headers: headers.clone(),
            body: b"request with headers".to_vec(),
            protocol: "HEADERS".to_string(),
            correlation_id: "headers-correlation".to_string(),
        };

        assert_eq!(request.headers.len(), 2);
        assert_eq!(
            request.headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            request.headers.get("content-type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_universal_response_with_headers() {
        let mut headers = HashMap::new();
        headers.insert("cache-control".to_string(), "no-cache".to_string());
        headers.insert("x-custom-header".to_string(), "custom-value".to_string());

        let response = UniversalResponse {
            status: 302,
            headers: headers.clone(),
            body: b"response with headers".to_vec(),
            protocol: "HEADERS".to_string(),
            correlation_id: "headers-response-correlation".to_string(),
        };

        assert_eq!(response.headers.len(), 2);
        assert_eq!(
            response.headers.get("cache-control"),
            Some(&"no-cache".to_string())
        );
        assert_eq!(
            response.headers.get("x-custom-header"),
            Some(&"custom-value".to_string())
        );
    }
}
