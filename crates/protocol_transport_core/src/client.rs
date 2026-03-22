//! Universal client abstractions for all protocols

use crate::error::ProtocolResult;
use crate::{
    HttpTransport, ProtocolError, ProtocolHandler, Transport, TransportError, TransportFactory,
    UniversalRequest, UniversalResponse,
};
use std::collections::HashMap;

/// **Universal Client** - Works with any protocol
pub struct UniversalClient<T: Transport, Req = serde_json::Value, Resp = serde_json::Value> {
    transport: T,
    protocol_handlers: HashMap<
        String,
        Box<dyn ProtocolHandler<Request = Req, Response = Resp, Error = ProtocolError>>,
    >,
}

impl<T: Transport, Req, Resp> UniversalClient<T, Req, Resp> {
    /// Create new universal client with given transport
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            protocol_handlers: HashMap::new(),
        }
    }

    /// Register protocol handler for the provided protocol name.
    pub fn register_protocol<H>(&mut self, protocol: &str, handler: H)
    where
        H: ProtocolHandler<Request = Req, Response = Resp, Error = ProtocolError> + 'static,
    {
        self.protocol_handlers
            .insert(protocol.to_string(), Box::new(handler));
    }

    /// Generic send API – passes already-constructed protocol request object through the transport layer.
    pub async fn send_request(&self, protocol: &str, request: Req) -> ProtocolResult<Resp>
    where
        Req: Send + Sync,
    {
        // Fetch a handler for the desired protocol
        let handler = self
            .protocol_handlers
            .get(protocol)
            .ok_or_else(|| ProtocolError::UnsupportedProtocol(protocol.to_string()))?;

        // Encode request into universal representation
        let universal_request = handler.encode_request(&request)?;

        // Execute transport call
        let universal_response = self
            .transport
            .send(universal_request)
            .await
            .map_err(ProtocolError::Transport)?;

        // Decode response back into protocol-specific form
        let protocol_response = handler.decode_response(&universal_response)?;

        Ok(protocol_response)
    }

    /// Health check
    pub async fn health_check(&self) -> ProtocolResult<()> {
        self.transport
            .health_check()
            .await
            .map_err(ProtocolError::Transport)
    }
}

/// **JSON-specific convenience helpers**
impl<T: Transport> UniversalClient<T, serde_json::Value, serde_json::Value> {
    /// Convenience helper that mirrors the previous JSON-centric API.
    pub async fn send_request_json(
        &self,
        protocol: &str,
        method: &str,
        uri: &str,
        params: serde_json::Value,
    ) -> ProtocolResult<serde_json::Value> {
        // Get protocol handler
        let handler = self
            .protocol_handlers
            .get(protocol)
            .ok_or_else(|| ProtocolError::UnsupportedProtocol(protocol.to_string()))?;

        // Build a JSON-RPC-like envelope that most of our early protocols expect.
        let protocol_request = serde_json::json!({
            "method": method,
            "uri": uri,
            "params": params,
        });

        // Encode
        let universal_request = handler.encode_request(&protocol_request)?;

        // Transport
        let universal_response = self
            .transport
            .send(universal_request)
            .await
            .map_err(ProtocolError::Transport)?;

        // Decode
        let result = handler.decode_response(&universal_response)?;
        Ok(result)
    }
}

/// **HTTP Client** - Convenience wrapper for HTTP transport
pub struct HttpClient {
    inner: UniversalClient<HttpTransport>,
}

impl HttpClient {
    /// Create HTTP client with default transport
    pub fn new() -> Self {
        Self {
            inner: UniversalClient::new(TransportFactory::http()),
        }
    }

    /// Create HTTP client for A2A protocol
    pub fn a2a() -> Self {
        Self {
            inner: UniversalClient::new(TransportFactory::a2a_http()),
        }
    }

    /// Create HTTP client for MCP protocol
    pub fn mcp() -> Self {
        Self {
            inner: UniversalClient::new(TransportFactory::mcp_http()),
        }
    }

    /// Create HTTP client for REST
    pub fn rest() -> Self {
        Self {
            inner: UniversalClient::new(TransportFactory::rest_http()),
        }
    }

    /// Register protocol handler
    pub fn register_protocol<H>(&mut self, protocol: &str, handler: H)
    where
        H: ProtocolHandler<
                Request = serde_json::Value,
                Response = serde_json::Value,
                Error = ProtocolError,
            > + 'static,
    {
        self.inner.register_protocol(protocol, handler);
    }

    /// Send request
    pub async fn send(
        &self,
        protocol: &str,
        method: &str,
        uri: &str,
        params: serde_json::Value,
    ) -> ProtocolResult<serde_json::Value> {
        self.inner
            .send_request_json(protocol, method, uri, params)
            .await
    }

    /// Health check
    pub async fn health_check(&self) -> ProtocolResult<()> {
        self.inner.health_check().await
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

/// **Client Builder** - Fluent interface for client configuration
pub struct ClientBuilder {
    transport_type: TransportType,
    protocol_configs: HashMap<String, ProtocolConfig>,
}

#[derive(Debug, Clone)]
enum TransportType {
    Http,
    A2AHttp,
    McpHttp,
    RestHttp,
}

#[derive(Debug, Clone)]
struct ProtocolConfig {
    enabled: bool,
    base_url: Option<String>,
    headers: HashMap<String, String>,
}

impl ClientBuilder {
    /// Create new builder
    pub fn new() -> Self {
        Self {
            transport_type: TransportType::Http,
            protocol_configs: HashMap::new(),
        }
    }

    /// Use HTTP transport
    pub fn http(mut self) -> Self {
        self.transport_type = TransportType::Http;
        self
    }

    /// Use A2A HTTP transport
    pub fn a2a_http(mut self) -> Self {
        self.transport_type = TransportType::A2AHttp;
        self
    }

    /// Use MCP HTTP transport
    pub fn mcp_http(mut self) -> Self {
        self.transport_type = TransportType::McpHttp;
        self
    }

    /// Use REST HTTP transport
    pub fn rest_http(mut self) -> Self {
        self.transport_type = TransportType::RestHttp;
        self
    }

    /// Enable protocol with configuration
    pub fn enable_protocol(mut self, protocol: &str, base_url: Option<String>) -> Self {
        let config = ProtocolConfig {
            enabled: true,
            base_url,
            headers: HashMap::new(),
        };
        self.protocol_configs.insert(protocol.to_string(), config);
        self
    }

    /// Build the client. Fails if a protocol was enabled but the caller did not register a handler.
    pub fn build(self) -> ProtocolResult<HttpClient> {
        let transport = match self.transport_type {
            TransportType::Http => TransportFactory::http(),
            TransportType::A2AHttp => TransportFactory::a2a_http(),
            TransportType::McpHttp => TransportFactory::mcp_http(),
            TransportType::RestHttp => TransportFactory::rest_http(),
        };

        let client = HttpClient {
            inner: UniversalClient::new(transport),
        };

        // Since we do not automatically create handlers yet, surface mis-configuration early.
        if !self.protocol_configs.is_empty() {
            // Build-time validation: we cannot proceed without concrete handler implementations
            // for the requested protocols.
            let enabled: Vec<String> = self.protocol_configs.keys().cloned().collect();
            return Err(ProtocolError::UnsupportedProtocol(format!(
                "Handlers missing for protocols: {:?}. Please register concrete handlers before building the client.",
                enabled
            )));
        }

        Ok(client)
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let _client = HttpClient::new();
        let _a2a_client = HttpClient::a2a();
        let _mcp_client = HttpClient::mcp();
        let _rest_client = HttpClient::rest();
    }

    #[test]
    fn test_client_builder_error() {
        let result = ClientBuilder::new()
            .a2a_http()
            .enable_protocol("A2A", Some("http://localhost:8080".to_string()))
            .build();

        assert!(result.is_err());
    }

    // Add comprehensive tests for better coverage
    #[test]
    fn test_universal_client_new() {
        let transport = crate::TransportFactory::http();
        let client: UniversalClient<_, serde_json::Value, serde_json::Value> =
            UniversalClient::new(transport);
        assert!(client.protocol_handlers.is_empty());
    }

    #[test]
    fn test_universal_client_register_protocol() {
        let transport = crate::TransportFactory::http();
        let mut client: UniversalClient<_, serde_json::Value, serde_json::Value> =
            UniversalClient::new(transport);

        // Mock handler that always succeeds
        struct MockHandler;
        impl crate::ProtocolHandler for MockHandler {
            type Request = serde_json::Value;
            type Response = serde_json::Value;
            type Error = crate::ProtocolError;

            fn protocol_name(&self) -> &'static str {
                "MOCK"
            }

            fn encode_request(
                &self,
                request: &Self::Request,
            ) -> Result<crate::UniversalRequest, Self::Error> {
                Ok(crate::UniversalRequest {
                    method: "POST".to_string(),
                    uri: "/mock".to_string(),
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(request).unwrap(),
                    protocol: "MOCK".to_string(),
                    correlation_id: uuid::Uuid::new_v4().to_string(),
                })
            }

            fn decode_request(
                &self,
                _universal: &crate::UniversalRequest,
            ) -> Result<Self::Request, Self::Error> {
                Ok(serde_json::json!({"decoded": true}))
            }

            fn encode_response(
                &self,
                response: &Self::Response,
            ) -> Result<crate::UniversalResponse, Self::Error> {
                Ok(crate::UniversalResponse {
                    status: 200,
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(response).unwrap(),
                    protocol: "MOCK".to_string(),
                    correlation_id: uuid::Uuid::new_v4().to_string(),
                })
            }

            fn decode_response(
                &self,
                universal: &crate::UniversalResponse,
            ) -> Result<Self::Response, Self::Error> {
                serde_json::from_slice(&universal.body)
                    .map_err(|e| crate::ProtocolError::Serialization(e))
            }
        }

        client.register_protocol("MOCK", MockHandler);
        assert_eq!(client.protocol_handlers.len(), 1);
        assert!(client.protocol_handlers.contains_key("MOCK"));
    }

    #[test]
    fn test_http_client_variants() {
        let _http_client = HttpClient::new();
        let _a2a_client = HttpClient::a2a();
        let _mcp_client = HttpClient::mcp();
        let _rest_client = HttpClient::rest();

        // Test default
        let _default_client = HttpClient::default();
        let _new_client = HttpClient::new();
        // Both should be functionally equivalent
    }

    #[test]
    fn test_client_builder_methods() {
        let builder = ClientBuilder::new();

        let _http_builder = builder.http();
        let _a2a_builder = ClientBuilder::new().a2a_http();
        let _mcp_builder = ClientBuilder::new().mcp_http();
        let _rest_builder = ClientBuilder::new().rest_http();

        // Test enable_protocol
        let enabled_builder =
            ClientBuilder::new().enable_protocol("TEST", Some("http://test.com".to_string()));

        // Should error when trying to build without handlers
        let result = enabled_builder.build();
        assert!(result.is_err());

        match result.err().unwrap() {
            crate::ProtocolError::UnsupportedProtocol(msg) => {
                assert!(msg.contains("Handlers missing"));
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[test]
    fn test_client_builder_multiple_protocols() {
        let result = ClientBuilder::new()
            .http()
            .enable_protocol("A2A", Some("http://a2a.com".to_string()))
            .enable_protocol("MCP", Some("http://mcp.com".to_string()))
            .enable_protocol("REST", None)
            .build();

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::ProtocolError::UnsupportedProtocol(msg) => {
                assert!(msg.contains("A2A"));
                assert!(msg.contains("MCP"));
                assert!(msg.contains("REST"));
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[test]
    fn test_client_builder_default() {
        let builder1 = ClientBuilder::new();
        let builder2 = ClientBuilder::default();

        // Both should build to empty clients successfully
        let client1 = builder1.build().unwrap();
        let client2 = builder2.build().unwrap();

        // Both should have empty protocol handlers initially
        assert!(client1.inner.protocol_handlers.is_empty());
        assert!(client2.inner.protocol_handlers.is_empty());
    }

    #[test]
    fn test_protocol_config_debug_clone() {
        let config = ProtocolConfig {
            enabled: true,
            base_url: Some("http://test.com".to_string()),
            headers: std::collections::HashMap::new(),
        };

        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("enabled"));
        assert!(debug_str.contains("http://test.com"));

        let cloned = config.clone();
        assert_eq!(config.enabled, cloned.enabled);
        assert_eq!(config.base_url, cloned.base_url);
        assert_eq!(config.headers, cloned.headers);
    }

    #[test]
    fn test_transport_type_debug_clone() {
        let transport_type = TransportType::Http;
        let debug_str = format!("{:?}", transport_type);
        assert!(debug_str.contains("Http"));

        let cloned = transport_type.clone();
        assert_eq!(format!("{:?}", transport_type), format!("{:?}", cloned));

        // Test all variants
        let a2a_type = TransportType::A2AHttp;
        let mcp_type = TransportType::McpHttp;
        let rest_type = TransportType::RestHttp;

        assert!(format!("{:?}", a2a_type).contains("A2AHttp"));
        assert!(format!("{:?}", mcp_type).contains("McpHttp"));
        assert!(format!("{:?}", rest_type).contains("RestHttp"));
    }

    #[test]
    fn test_http_client_register_protocol() {
        let mut client = HttpClient::new();

        // Mock handler for testing
        struct SimpleHandler;
        impl crate::ProtocolHandler for SimpleHandler {
            type Request = serde_json::Value;
            type Response = serde_json::Value;
            type Error = crate::ProtocolError;

            fn protocol_name(&self) -> &'static str {
                "SIMPLE"
            }

            fn encode_request(
                &self,
                request: &Self::Request,
            ) -> Result<crate::UniversalRequest, Self::Error> {
                Ok(crate::UniversalRequest {
                    method: "POST".to_string(),
                    uri: "/simple".to_string(),
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(request).unwrap(),
                    protocol: "SIMPLE".to_string(),
                    correlation_id: "simple-correlation".to_string(),
                })
            }

            fn decode_request(
                &self,
                _universal: &crate::UniversalRequest,
            ) -> Result<Self::Request, Self::Error> {
                Ok(serde_json::json!({"simple": true}))
            }

            fn encode_response(
                &self,
                response: &Self::Response,
            ) -> Result<crate::UniversalResponse, Self::Error> {
                Ok(crate::UniversalResponse {
                    status: 200,
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(response).unwrap(),
                    protocol: "SIMPLE".to_string(),
                    correlation_id: "simple-response".to_string(),
                })
            }

            fn decode_response(
                &self,
                universal: &crate::UniversalResponse,
            ) -> Result<Self::Response, Self::Error> {
                serde_json::from_slice(&universal.body)
                    .map_err(|e| crate::ProtocolError::Serialization(e))
            }
        }

        client.register_protocol("SIMPLE", SimpleHandler);
        assert_eq!(client.inner.protocol_handlers.len(), 1);
    }

    #[test]
    fn test_client_builder_empty_build() {
        // Should succeed when no protocols are enabled
        let result = ClientBuilder::new().build();
        assert!(result.is_ok());

        let client = result.unwrap();
        assert!(client.inner.protocol_handlers.is_empty());
    }

    #[test]
    fn test_client_builder_all_transport_types() {
        // Test that all transport type builders work
        let _http = ClientBuilder::new().http().build().unwrap();
        let _a2a = ClientBuilder::new().a2a_http().build().unwrap();
        let _mcp = ClientBuilder::new().mcp_http().build().unwrap();
        let _rest = ClientBuilder::new().rest_http().build().unwrap();

        // All should build successfully when no protocols are enabled
    }

    #[test]
    fn test_protocol_config_all_fields() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("test-header".to_string(), "test-value".to_string());

        let config = ProtocolConfig {
            enabled: false,
            base_url: None,
            headers,
        };

        assert!(!config.enabled);
        assert!(config.base_url.is_none());
        assert_eq!(
            config.headers.get("test-header"),
            Some(&"test-value".to_string())
        );
    }

    // ===== NEW COMPREHENSIVE TESTS FOR MISSING COVERAGE =====

    #[tokio::test]
    async fn test_universal_client_send_request_missing_protocol() {
        let transport = crate::TransportFactory::http();
        let client: UniversalClient<_, serde_json::Value, serde_json::Value> =
            UniversalClient::new(transport);

        let request = serde_json::json!({"test": "data"});
        let result = client.send_request("MISSING_PROTOCOL", request).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::ProtocolError::UnsupportedProtocol(protocol) => {
                assert_eq!(protocol, "MISSING_PROTOCOL");
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[tokio::test]
    async fn test_universal_client_health_check() {
        let transport = crate::TransportFactory::http();
        let client: UniversalClient<_, serde_json::Value, serde_json::Value> =
            UniversalClient::new(transport);

        // Health check should succeed since HttpTransport just returns Ok(())
        let result = client.health_check().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_universal_client_send_request_json_missing_protocol() {
        let transport = crate::TransportFactory::http();
        let client: UniversalClient<_, serde_json::Value, serde_json::Value> =
            UniversalClient::new(transport);

        let result = client
            .send_request_json(
                "MISSING_JSON",
                "test_method",
                "/test/uri",
                serde_json::json!({"param": "value"}),
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::ProtocolError::UnsupportedProtocol(protocol) => {
                assert_eq!(protocol, "MISSING_JSON");
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[tokio::test]
    async fn test_http_client_send_missing_protocol() {
        let client = HttpClient::new();

        let result = client
            .send(
                "NONEXISTENT",
                "POST",
                "/test",
                serde_json::json!({"data": "test"}),
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            crate::ProtocolError::UnsupportedProtocol(protocol) => {
                assert_eq!(protocol, "NONEXISTENT");
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[tokio::test]
    async fn test_http_client_health_check() {
        let client = HttpClient::new();

        // Health check should succeed since HttpTransport just returns Ok(())
        let result = client.health_check().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_mock_protocol_handlers_for_coverage() {
        // This test exercises the mock handler methods that weren't covered

        struct TestMockHandler;
        impl crate::ProtocolHandler for TestMockHandler {
            type Request = serde_json::Value;
            type Response = serde_json::Value;
            type Error = crate::ProtocolError;

            fn protocol_name(&self) -> &'static str {
                "TEST_MOCK"
            }

            fn encode_request(
                &self,
                request: &Self::Request,
            ) -> Result<crate::UniversalRequest, Self::Error> {
                Ok(crate::UniversalRequest {
                    method: "POST".to_string(),
                    uri: "/test_mock".to_string(),
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(request).unwrap(),
                    protocol: "TEST_MOCK".to_string(),
                    correlation_id: "test-mock-correlation".to_string(),
                })
            }

            fn decode_request(
                &self,
                _universal: &crate::UniversalRequest,
            ) -> Result<Self::Request, Self::Error> {
                Ok(serde_json::json!({"decoded_test": true}))
            }

            fn encode_response(
                &self,
                response: &Self::Response,
            ) -> Result<crate::UniversalResponse, Self::Error> {
                Ok(crate::UniversalResponse {
                    status: 200,
                    headers: std::collections::HashMap::new(),
                    body: serde_json::to_vec(response).unwrap(),
                    protocol: "TEST_MOCK".to_string(),
                    correlation_id: "test-mock-response".to_string(),
                })
            }

            fn decode_response(
                &self,
                universal: &crate::UniversalResponse,
            ) -> Result<Self::Response, Self::Error> {
                serde_json::from_slice(&universal.body)
                    .map_err(|e| crate::ProtocolError::Serialization(e))
            }
        }

        let handler = TestMockHandler;

        // Test protocol_name
        assert_eq!(handler.protocol_name(), "TEST_MOCK");

        // Test encode_request
        let request = serde_json::json!({"test": "request"});
        let universal_req = handler.encode_request(&request).unwrap();
        assert_eq!(universal_req.protocol, "TEST_MOCK");
        assert_eq!(universal_req.uri, "/test_mock");

        // Test decode_request
        let decoded = handler.decode_request(&universal_req).unwrap();
        assert_eq!(decoded["decoded_test"], true);

        // Test encode_response
        let response = serde_json::json!({"test": "response"});
        let universal_resp = handler.encode_response(&response).unwrap();
        assert_eq!(universal_resp.status, 200);
        assert_eq!(universal_resp.protocol, "TEST_MOCK");

        // Test decode_response
        let decoded_resp = handler.decode_response(&universal_resp).unwrap();
        assert_eq!(decoded_resp["test"], "response");
    }

    #[test]
    fn test_protocol_handler_error_cases() {
        struct ErrorMockHandler;
        impl crate::ProtocolHandler for ErrorMockHandler {
            type Request = serde_json::Value;
            type Response = serde_json::Value;
            type Error = crate::ProtocolError;

            fn protocol_name(&self) -> &'static str {
                "ERROR_MOCK"
            }

            fn encode_request(
                &self,
                _request: &Self::Request,
            ) -> Result<crate::UniversalRequest, Self::Error> {
                Err(crate::ProtocolError::Internal(
                    "Encode request failed".to_string(),
                ))
            }

            fn decode_request(
                &self,
                _universal: &crate::UniversalRequest,
            ) -> Result<Self::Request, Self::Error> {
                Err(crate::ProtocolError::Internal(
                    "Decode request failed".to_string(),
                ))
            }

            fn encode_response(
                &self,
                _response: &Self::Response,
            ) -> Result<crate::UniversalResponse, Self::Error> {
                Err(crate::ProtocolError::Internal(
                    "Encode response failed".to_string(),
                ))
            }

            fn decode_response(
                &self,
                _universal: &crate::UniversalResponse,
            ) -> Result<Self::Response, Self::Error> {
                Err(crate::ProtocolError::Internal(
                    "Decode response failed".to_string(),
                ))
            }
        }

        let handler = ErrorMockHandler;
        let request = serde_json::json!({"test": true});
        let universal_req = crate::UniversalRequest {
            method: "POST".to_string(),
            uri: "/test".to_string(),
            headers: std::collections::HashMap::new(),
            body: b"test".to_vec(),
            protocol: "ERROR_MOCK".to_string(),
            correlation_id: "error-test".to_string(),
        };
        let universal_resp = crate::UniversalResponse {
            status: 500,
            headers: std::collections::HashMap::new(),
            body: b"error".to_vec(),
            protocol: "ERROR_MOCK".to_string(),
            correlation_id: "error-response".to_string(),
        };

        // Test all error cases
        assert!(handler.encode_request(&request).is_err());
        assert!(handler.decode_request(&universal_req).is_err());
        assert!(handler.encode_response(&request).is_err());
        assert!(handler.decode_response(&universal_resp).is_err());
    }

    #[test]
    fn test_client_builder_enable_protocol_variations() {
        // Test enable_protocol with different configurations
        let builder1 =
            ClientBuilder::new().enable_protocol("PROTO1", Some("http://proto1.com".to_string()));

        let builder2 = ClientBuilder::new().enable_protocol("PROTO2", None);

        let builder3 = ClientBuilder::new()
            .a2a_http()
            .enable_protocol("PROTO3", Some("https://secure.proto3.com".to_string()));

        // All should fail when built without handlers
        assert!(builder1.build().is_err());
        assert!(builder2.build().is_err());
        assert!(builder3.build().is_err());
    }

    #[test]
    fn test_protocol_config_with_custom_headers() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("authorization".to_string(), "Bearer token123".to_string());
        headers.insert("x-api-key".to_string(), "secret-key".to_string());

        let config = ProtocolConfig {
            enabled: true,
            base_url: Some("https://api.example.com".to_string()),
            headers: headers.clone(),
        };

        assert!(config.enabled);
        assert_eq!(config.base_url, Some("https://api.example.com".to_string()));
        assert_eq!(config.headers.len(), 2);
        assert_eq!(
            config.headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            config.headers.get("x-api-key"),
            Some(&"secret-key".to_string())
        );

        // Test clone
        let cloned_config = config.clone();
        assert_eq!(config.headers, cloned_config.headers);
    }

    #[test]
    fn test_transport_type_all_variants() {
        let types = vec![
            TransportType::Http,
            TransportType::A2AHttp,
            TransportType::McpHttp,
            TransportType::RestHttp,
        ];

        for transport_type in types {
            let debug_str = format!("{:?}", transport_type);
            assert!(!debug_str.is_empty());

            // Test clone
            let cloned = transport_type.clone();
            assert_eq!(format!("{:?}", transport_type), format!("{:?}", cloned));
        }
    }

    #[test]
    fn test_client_builder_transport_switching() {
        // Test switching between transport types
        let builder = ClientBuilder::new()
            .http()
            .a2a_http()
            .mcp_http()
            .rest_http()
            .http(); // Back to HTTP

        let client = builder.build().unwrap();
        assert!(client.inner.protocol_handlers.is_empty());
    }

    #[test]
    fn test_edge_cases_and_empty_values() {
        // Test empty protocol name
        let result = ClientBuilder::new().enable_protocol("", None).build();
        assert!(result.is_err());

        // Test protocol with empty base URL vs None
        let builder1 = ClientBuilder::new().enable_protocol("EMPTY_URL", Some("".to_string()));
        let builder2 = ClientBuilder::new().enable_protocol("NO_URL", None);

        assert!(builder1.build().is_err());
        assert!(builder2.build().is_err());
    }
}
