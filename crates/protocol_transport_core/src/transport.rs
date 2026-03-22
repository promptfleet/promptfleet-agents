//! Universal HTTP transport implementation

use crate::{Transport, TransportError, UniversalRequest, UniversalResponse};
use std::collections::HashMap;

// WASM-specific imports
#[cfg(target_arch = "wasm32")]
use spin_sdk::http::{Method, Request as SpinRequest, Response as SpinResponse};

// Native-specific imports
#[cfg(not(target_arch = "wasm32"))]
use reqwest::{Client as ReqwestClient, Method as ReqwestMethod};

/// **HTTP Transport** - Common implementation for all protocols
pub struct HttpTransport {
    /// Default headers to include in all requests
    default_headers: HashMap<String, String>,
    /// Native HTTP client (only for non-WASM targets)
    #[cfg(not(target_arch = "wasm32"))]
    client: ReqwestClient,
}

impl HttpTransport {
    /// Create new HTTP transport
    pub fn new() -> Self {
        let mut default_headers = HashMap::new();
        default_headers.insert(
            "User-Agent".to_string(),
            "SpinKube-Protocol-Client/1.0".to_string(),
        );

        Self {
            default_headers,
            #[cfg(not(target_arch = "wasm32"))]
            client: ReqwestClient::new(),
        }
    }

    /// Add default header for all requests
    pub fn with_default_header(mut self, key: String, value: String) -> Self {
        self.default_headers.insert(key, value);
        self
    }

    /// Convert HTTP method string to appropriate method type (WASM version)
    #[cfg(target_arch = "wasm32")]
    fn parse_method(method: &str) -> Method {
        match method.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            _ => Method::Post, // Default fallback
        }
    }

    /// Convert HTTP method string to appropriate method type (Native version)
    #[cfg(not(target_arch = "wasm32"))]
    fn parse_method(method: &str) -> ReqwestMethod {
        match method.to_uppercase().as_str() {
            "GET" => ReqwestMethod::GET,
            "POST" => ReqwestMethod::POST,
            "PUT" => ReqwestMethod::PUT,
            "DELETE" => ReqwestMethod::DELETE,
            "PATCH" => ReqwestMethod::PATCH,
            "HEAD" => ReqwestMethod::HEAD,
            "OPTIONS" => ReqwestMethod::OPTIONS,
            _ => ReqwestMethod::POST, // Default fallback
        }
    }

    /// Build Spin HTTP request from universal request (WASM version)
    #[cfg(target_arch = "wasm32")]
    fn build_spin_request(
        &self,
        universal: &UniversalRequest,
    ) -> Result<SpinRequest, TransportError> {
        let method = Self::parse_method(&universal.method);

        let mut builder = SpinRequest::builder();
        builder.method(method).uri(&universal.uri);

        // Add default headers
        for (key, value) in &self.default_headers {
            builder.header(key, value);
        }

        // Add request-specific headers
        for (key, value) in &universal.headers {
            builder.header(key, value);
        }

        // Add body
        let body = if universal.body.is_empty() {
            String::new()
        } else {
            String::from_utf8(universal.body.clone())
                .map_err(|e| TransportError::Serialization(format!("Invalid UTF-8 body: {}", e)))?
        };

        Ok(builder.body(body).build())
    }

    /// Convert Spin HTTP response to universal response (WASM version)
    #[cfg(target_arch = "wasm32")]
    fn build_universal_response(
        &self,
        spin_response: SpinResponse,
        correlation_id: String,
        protocol: String,
    ) -> UniversalResponse {
        let mut headers = HashMap::new();

        // Extract headers from Spin response
        for (name, value) in spin_response.headers() {
            if let Some(value_str) = value.as_str() {
                headers.insert(name.to_string(), value_str.to_string());
            }
        }

        UniversalResponse {
            status: *spin_response.status(),
            headers,
            body: spin_response.body().to_vec(),
            correlation_id,
            protocol,
        }
    }
}

impl Transport for HttpTransport {
    /// Send request using target-appropriate HTTP implementation
    async fn send(&self, request: UniversalRequest) -> Result<UniversalResponse, TransportError> {
        #[cfg(target_arch = "wasm32")]
        {
            // WASM implementation using Spin SDK
            let spin_request = self.build_spin_request(&request)?;

            let spin_response: SpinResponse = spin_sdk::http::send(spin_request)
                .await
                .map_err(|e| TransportError::Network(format!("Spin HTTP error: {}", e)))?;

            // Capture headers for error reporting
            let mut response_headers: HashMap<String, String> = HashMap::new();
            for (name, value) in spin_response.headers() {
                if let Some(v) = value.as_str() {
                    response_headers.insert(name.to_string(), v.to_string());
                }
            }

            // Check for HTTP errors (>= 400)
            let status = *spin_response.status();
            if status >= 400 {
                return Err(TransportError::Http {
                    status,
                    message: format!("HTTP {} response", status),
                    body: Some(spin_response.body().to_vec()),
                    headers: Some(response_headers),
                });
            }

            // Convert to universal response
            let universal_response = self.build_universal_response(
                spin_response,
                request.correlation_id,
                request.protocol,
            );

            Ok(universal_response)
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native implementation using Reqwest
            let method = Self::parse_method(&request.method);

            let mut req_builder = self.client.request(method, &request.uri);

            // Add default headers
            for (key, value) in &self.default_headers {
                req_builder = req_builder.header(key, value);
            }

            // Add request-specific headers
            for (key, value) in &request.headers {
                req_builder = req_builder.header(key, value);
            }

            // Add body
            if !request.body.is_empty() {
                req_builder = req_builder.body(request.body.clone());
            }

            // Send request
            let response = req_builder
                .send()
                .await
                .map_err(|e| TransportError::Network(format!("Reqwest error: {}", e)))?;

            let status = response.status().as_u16();

            // Extract headers
            let mut resp_headers = HashMap::new();
            for (name, value) in response.headers() {
                if let Ok(value_str) = value.to_str() {
                    resp_headers.insert(name.to_string(), value_str.to_string());
                }
            }

            // Get body
            let body = response
                .bytes()
                .await
                .map_err(|e| {
                    TransportError::Network(format!("Failed to read response body: {}", e))
                })?
                .to_vec();

            // Check for HTTP errors
            if status >= 400 {
                return Err(TransportError::Http {
                    status,
                    message: format!("HTTP {} response", status),
                    body: Some(body.clone()),
                    headers: Some(resp_headers.clone()),
                });
            }

            Ok(UniversalResponse {
                status,
                headers: resp_headers,
                body,
                correlation_id: request.correlation_id,
                protocol: request.protocol,
            })
        }
    }

    async fn health_check(&self) -> Result<(), TransportError> {
        // Basic health check - both implementations can use this simple version
        Ok(())
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// **SSE Transport** - For external MCP servers (web-standard)
pub struct SseTransport {
    /// SSE endpoint URL
    endpoint_url: String,
    /// Default headers for SSE connection
    default_headers: HashMap<String, String>,
    /// HTTP client for sending requests
    #[cfg(not(target_arch = "wasm32"))]
    client: ReqwestClient,
}

impl SseTransport {
    /// Create new SSE transport for MCP external servers
    pub fn new(endpoint_url: &str) -> Self {
        let mut default_headers = HashMap::new();
        default_headers.insert("Accept".to_string(), "text/event-stream".to_string());
        default_headers.insert("Cache-Control".to_string(), "no-cache".to_string());
        default_headers.insert("Connection".to_string(), "keep-alive".to_string());
        default_headers.insert(
            "User-Agent".to_string(),
            "PromptFleet-MCP-Client/1.0".to_string(),
        );

        Self {
            endpoint_url: endpoint_url.to_string(),
            default_headers,
            #[cfg(not(target_arch = "wasm32"))]
            client: ReqwestClient::new(),
        }
    }

    /// Add authentication header for SSE connection
    pub fn with_auth_header(mut self, auth_header: String) -> Self {
        self.default_headers
            .insert("Authorization".to_string(), auth_header);
        self
    }

    /// Build SSE connection request (WASM version)
    #[cfg(target_arch = "wasm32")]
    fn build_sse_request(
        &self,
        universal: &UniversalRequest,
    ) -> Result<SpinRequest, TransportError> {
        let mut builder = SpinRequest::builder();
        builder.method(Method::Get).uri(&self.endpoint_url);

        // Add SSE headers
        for (key, value) in &self.default_headers {
            builder.header(key, value);
        }

        // Add request-specific headers (auth, etc.)
        for (key, value) in &universal.headers {
            builder.header(key, value);
        }

        Ok(builder.body(String::new()).build())
    }

    /// Send JSON-RPC message via POST to SSE endpoint /messages (WASM version)
    #[cfg(target_arch = "wasm32")]
    fn build_message_request(
        &self,
        universal: &UniversalRequest,
    ) -> Result<SpinRequest, TransportError> {
        // SSE typically uses /messages endpoint for bidirectional communication
        let message_url = format!("{}/messages", self.endpoint_url.trim_end_matches("/sse"));

        let mut builder = SpinRequest::builder();
        builder.method(Method::Post).uri(&message_url);

        // JSON-RPC content type
        builder.header("Content-Type", "application/json");

        // Add auth headers
        for (key, value) in &self.default_headers {
            if key != "Accept" && key != "Cache-Control" && key != "Connection" {
                builder.header(key, value);
            }
        }

        // Add request-specific headers
        for (key, value) in &universal.headers {
            builder.header(key, value);
        }

        // Add JSON-RPC body
        let body = if universal.body.is_empty() {
            String::new()
        } else {
            String::from_utf8(universal.body.clone())
                .map_err(|e| TransportError::Serialization(format!("Invalid UTF-8 body: {}", e)))?
        };

        Ok(builder.body(body).build())
    }
}

impl Transport for SseTransport {
    async fn send(&self, request: UniversalRequest) -> Result<UniversalResponse, TransportError> {
        #[cfg(target_arch = "wasm32")]
        {
            // WASM implementation using Spin SDK
            let spin_request = self.build_message_request(&request)?;

            let spin_response: SpinResponse = spin_sdk::http::send(spin_request)
                .await
                .map_err(|e| TransportError::Network(format!("SSE HTTP error: {}", e)))?;

            // Capture headers for error reporting
            let mut response_headers: HashMap<String, String> = HashMap::new();
            for (name, value) in spin_response.headers() {
                if let Some(v) = value.as_str() {
                    response_headers.insert(name.to_string(), v.to_string());
                }
            }

            // Check for HTTP errors
            let status = *spin_response.status();
            if status >= 400 {
                return Err(TransportError::Http {
                    status,
                    message: format!("SSE HTTP {} response", status),
                    body: Some(spin_response.body().to_vec()),
                    headers: Some(response_headers),
                });
            }

            // Convert to universal response
            let universal_response = UniversalResponse {
                status,
                headers: response_headers,
                body: spin_response.body().to_vec(),
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            };

            Ok(universal_response)
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native implementation using Reqwest
            let message_url = format!("{}/messages", self.endpoint_url.trim_end_matches("/sse"));

            let mut req_builder = self.client.post(&message_url);

            // Add headers
            req_builder = req_builder.header("Content-Type", "application/json");
            for (key, value) in &self.default_headers {
                if key != "Accept" && key != "Cache-Control" && key != "Connection" {
                    req_builder = req_builder.header(key, value);
                }
            }
            for (key, value) in &request.headers {
                req_builder = req_builder.header(key, value);
            }

            // Add body
            if !request.body.is_empty() {
                req_builder = req_builder.body(request.body.clone());
            }

            // Send request
            let response = req_builder
                .send()
                .await
                .map_err(|e| TransportError::Network(format!("SSE Reqwest error: {}", e)))?;

            let status = response.status().as_u16();

            // Extract headers
            let mut resp_headers = HashMap::new();
            for (name, value) in response.headers() {
                if let Ok(value_str) = value.to_str() {
                    resp_headers.insert(name.to_string(), value_str.to_string());
                }
            }

            // Get body
            let body = response
                .bytes()
                .await
                .map_err(|e| {
                    TransportError::Network(format!("Failed to read SSE response body: {}", e))
                })?
                .to_vec();

            // Check for HTTP errors
            if status >= 400 {
                return Err(TransportError::Http {
                    status,
                    message: format!("SSE HTTP {} response", status),
                    body: Some(body.clone()),
                    headers: Some(resp_headers.clone()),
                });
            }

            Ok(UniversalResponse {
                status,
                headers: resp_headers,
                body,
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            })
        }
    }

    async fn health_check(&self) -> Result<(), TransportError> {
        #[cfg(target_arch = "wasm32")]
        {
            // WASM health check using Spin SDK
            let health_request = SpinRequest::builder()
                .method(Method::Get)
                .uri(&self.endpoint_url)
                .header("Accept", "text/event-stream")
                .body(String::new())
                .build();

            let response: SpinResponse = spin_sdk::http::send(health_request)
                .await
                .map_err(|e| TransportError::Network(format!("SSE health check failed: {}", e)))?;

            if *response.status() >= 400 {
                return Err(TransportError::Http {
                    status: *response.status(),
                    message: "SSE endpoint unhealthy".to_string(),
                    body: None,
                    headers: None,
                });
            }

            Ok(())
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native health check using Reqwest
            let response = self
                .client
                .get(&self.endpoint_url)
                .header("Accept", "text/event-stream")
                .send()
                .await
                .map_err(|e| TransportError::Network(format!("SSE health check failed: {}", e)))?;

            if response.status().as_u16() >= 400 {
                return Err(TransportError::Http {
                    status: response.status().as_u16(),
                    message: "SSE endpoint unhealthy".to_string(),
                    body: None,
                    headers: None,
                });
            }

            Ok(())
        }
    }
}

/// **Transport Factory** - Easy creation of transports
pub struct TransportFactory;

impl TransportFactory {
    /// Create HTTP transport with common defaults
    pub fn http() -> HttpTransport {
        HttpTransport::new()
            .with_default_header("Content-Type".to_string(), "application/json".to_string())
    }

    /// Create HTTP transport for A2A protocol
    pub fn a2a_http() -> HttpTransport {
        HttpTransport::new()
            .with_default_header("Content-Type".to_string(), "application/json".to_string())
            .with_default_header("X-Protocol".to_string(), "A2A".to_string())
    }

    /// Create HTTP transport for MCP protocol (internal)
    pub fn mcp_http() -> HttpTransport {
        HttpTransport::new()
            .with_default_header("Content-Type".to_string(), "application/json".to_string())
            .with_default_header("X-Protocol".to_string(), "MCP".to_string())
    }

    /// Create SSE transport for external MCP servers (web-standard)
    pub fn mcp_sse(endpoint_url: &str) -> SseTransport {
        SseTransport::new(endpoint_url)
    }

    /// Create SSE transport with authentication for external MCP servers
    pub fn mcp_sse_auth(endpoint_url: &str, auth_token: &str) -> SseTransport {
        SseTransport::new(endpoint_url).with_auth_header(format!("Bearer {}", auth_token))
    }

    /// Create HTTP transport for REST
    pub fn rest_http() -> HttpTransport {
        HttpTransport::new()
            .with_default_header("Accept".to_string(), "application/json".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new();
        assert!(transport.default_headers.contains_key("User-Agent"));
    }

    #[test]
    fn test_transport_factory() {
        let http_transport = TransportFactory::http();
        assert!(http_transport.default_headers.contains_key("Content-Type"));

        let a2a_transport = TransportFactory::a2a_http();
        assert!(a2a_transport.default_headers.contains_key("X-Protocol"));
    }

    #[test]
    fn test_method_parsing() {
        #[cfg(target_arch = "wasm32")]
        {
            assert!(matches!(
                HttpTransport::parse_method("GET"),
                spin_sdk::http::Method::Get
            ));
            assert!(matches!(
                HttpTransport::parse_method("post"),
                spin_sdk::http::Method::Post
            ));
            assert!(matches!(
                HttpTransport::parse_method("PUT"),
                spin_sdk::http::Method::Put
            ));
            assert!(matches!(
                HttpTransport::parse_method("UNKNOWN"),
                spin_sdk::http::Method::Post
            ));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            assert!(matches!(
                HttpTransport::parse_method("GET"),
                reqwest::Method::GET
            ));
            assert!(matches!(
                HttpTransport::parse_method("POST"),
                reqwest::Method::POST
            ));
        }
    }

    #[test]
    fn test_http_transport_with_default_headers() {
        let transport = HttpTransport::new()
            .with_default_header("Custom-Header".to_string(), "Custom-Value".to_string());

        assert!(transport.default_headers.contains_key("Custom-Header"));
        assert_eq!(
            transport.default_headers.get("Custom-Header"),
            Some(&"Custom-Value".to_string())
        );
        assert!(transport.default_headers.contains_key("User-Agent"));
    }

    // Target-specific tests
    #[cfg(target_arch = "wasm32")]
    mod wasm_tests {
        use super::*;

        #[test]
        fn test_spin_request_building() {
            let transport = HttpTransport::new();
            let universal_request = crate::UniversalRequest {
                method: "GET".to_string(),
                uri: "/test".to_string(),
                headers: {
                    let mut headers = std::collections::HashMap::new();
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                    headers
                },
                body: b"test body".to_vec(),
                protocol: "TEST".to_string(),
                correlation_id: "test-correlation".to_string(),
            };

            let result = transport.build_spin_request(&universal_request);
            assert!(result.is_ok());

            let spin_request = result.unwrap();
            assert_eq!(spin_request.method(), &spin_sdk::http::Method::Get);
            assert_eq!(spin_request.path(), "/test");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod native_tests {
        use super::*;

        #[tokio::test]
        async fn test_native_http_transport() {
            let transport = HttpTransport::new();
            assert!(transport
                .client
                .get("https://httpbin.org/get")
                .build()
                .is_ok());
        }

        #[test]
        fn test_reqwest_method_parsing() {
            assert!(matches!(
                HttpTransport::parse_method("GET"),
                reqwest::Method::GET
            ));
            assert!(matches!(
                HttpTransport::parse_method("POST"),
                reqwest::Method::POST
            ));
        }
    }

    // Common tests for both targets
    #[test]
    fn test_transport_factory_all_methods() {
        let http_transport = TransportFactory::http();
        assert!(http_transport.default_headers.contains_key("Content-Type"));

        let a2a_transport = TransportFactory::a2a_http();
        assert!(a2a_transport.default_headers.contains_key("X-Protocol"));
        assert_eq!(
            a2a_transport.default_headers.get("X-Protocol"),
            Some(&"A2A".to_string())
        );

        let mcp_transport = TransportFactory::mcp_http();
        assert!(mcp_transport.default_headers.contains_key("X-Protocol"));
        assert_eq!(
            mcp_transport.default_headers.get("X-Protocol"),
            Some(&"MCP".to_string())
        );

        let rest_transport = TransportFactory::rest_http();
        assert!(rest_transport.default_headers.contains_key("Accept"));

        let sse_transport = TransportFactory::mcp_sse("http://example.com/sse");
        assert_eq!(sse_transport.endpoint_url, "http://example.com/sse");

        let sse_auth_transport = TransportFactory::mcp_sse_auth("http://example.com", "token123");
        assert!(sse_auth_transport
            .default_headers
            .contains_key("Authorization"));
        assert_eq!(
            sse_auth_transport.default_headers.get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
    }
}
