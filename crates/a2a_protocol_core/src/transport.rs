//! A2A Transport Abstraction
//!
//! Defines the transport abstraction for A2A protocol communication.
//! This trait provides a clean interface between the A2A protocol logic
//! and the underlying transport implementation (HTTP, WebSocket, etc.).

use crate::A2AResult;
#[cfg(test)]
use crate::A2AError;
use protocol_transport_core::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::collections::HashMap;

/// A2A Transport trait
///
/// Defines the interface for sending A2A protocol messages over various
/// transport mechanisms. Implementations handle the actual network
/// communication while the protocol core handles the A2A semantics.
///
/// # Design Principles
///
/// - **Transport Agnostic**: Works with HTTP, WebSocket, gRPC, etc.
/// - **JSON-RPC Foundation**: Uses JSON-RPC 2.0 message types
/// - **Async Compatible**: Supports both sync and async implementations
/// - **Error Transparent**: Transport errors are mapped to A2A errors
///
/// # Examples
///
/// ```rust
/// use a2a_protocol_core::{A2ATransport, JsonRpcRequest, JsonRpcResponse, JsonRpcNotification, A2AResult};
/// use serde_json::json;
///
/// // Mock transport implementation
/// struct MockTransport;
///
/// impl A2ATransport for MockTransport {
///     async fn send_request(&self, request: JsonRpcRequest) -> A2AResult<JsonRpcResponse> {
///         // Implementation sends request over network
///         Ok(JsonRpcResponse::success(request.id, json!({"pong": true})))
///     }
///
///     async fn send_notification(&self, notification: JsonRpcNotification) -> A2AResult<()> {
///         // Implementation sends notification (fire-and-forget)
///         Ok(())
///     }
///
///     async fn health_check(&self) -> A2AResult<()> {
///         // Check if transport is available
///         Ok(())
///     }
/// }
/// ```
// Intentional: WASM targets don't require Send on futures, so async-in-trait is fine.
#[allow(async_fn_in_trait)]
pub trait A2ATransport: Send + Sync {
    /// Send a JSON-RPC request and wait for response
    ///
    /// This method sends a request to a remote agent and waits for the response.
    /// The transport implementation handles the actual network communication,
    /// serialization, and correlation of requests with responses.
    ///
    /// # Arguments
    ///
    /// - `request`: The JSON-RPC 2.0 request to send
    ///
    /// # Returns
    ///
    /// - `Ok(JsonRpcResponse)`: The response from the remote agent
    /// - `Err(A2AError)`: Transport or protocol error
    async fn send_request(&self, request: JsonRpcRequest) -> A2AResult<JsonRpcResponse>;

    /// Send a JSON-RPC notification (fire-and-forget)
    ///
    /// This method sends a notification to a remote agent without waiting for
    /// a response. Notifications are used for logging, metrics, events, and
    /// other operations where the sender doesn't need confirmation.
    ///
    /// # Arguments
    ///
    /// - `notification`: The JSON-RPC 2.0 notification to send
    ///
    /// # Returns
    ///
    /// - `Ok(())`: Notification was sent successfully
    /// - `Err(A2AError)`: Transport error occurred
    async fn send_notification(&self, notification: JsonRpcNotification) -> A2AResult<()>;

    /// Check if the transport is available and healthy
    ///
    /// This method verifies that the transport can communicate with remote
    /// agents. It's used for health checks, circuit breakers, and load
    /// balancing decisions.
    ///
    /// # Returns
    ///
    /// - `Ok(())`: Transport is healthy and available
    /// - `Err(A2AError)`: Transport is unavailable or unhealthy
    async fn health_check(&self) -> A2AResult<()>;

    /// Get transport-specific metadata
    ///
    /// This method returns metadata about the transport implementation,
    /// such as connection info, capabilities, or configuration details.
    ///
    /// # Returns
    ///
    /// A map of metadata key-value pairs
    fn get_metadata(&self) -> HashMap<String, Value> {
        HashMap::new()
    }

    /// Get the transport type identifier
    ///
    /// Returns a string identifying the transport type (e.g., "http", "websocket").
    /// This is used for logging, monitoring, and debugging.
    fn transport_type(&self) -> &'static str {
        "unknown"
    }
}

/// A2A Transport Factory trait
///
/// Factory for creating transport instances for different agents or endpoints.
/// This enables connection pooling, load balancing, and dynamic transport
/// configuration.
// Intentional: WASM targets don't require Send on futures, so async-in-trait is fine.
#[allow(async_fn_in_trait)]
pub trait A2ATransportFactory: Send + Sync {
    /// Transport type created by this factory
    type Transport: A2ATransport;

    /// Create a transport instance for the specified agent
    ///
    /// # Arguments
    ///
    /// - `agent_id`: Target agent identifier
    /// - `endpoint`: Agent endpoint URL or connection string
    /// - `config`: Optional transport-specific configuration
    ///
    /// # Returns
    ///
    /// - `Ok(Transport)`: Successfully created transport
    /// - `Err(A2AError)`: Failed to create transport
    async fn create_transport(
        &self,
        agent_id: &str,
        endpoint: &str,
        config: Option<HashMap<String, Value>>,
    ) -> A2AResult<Self::Transport>;

    /// Get or create cached transport for agent
    ///
    /// This method may reuse existing connections for better performance.
    ///
    /// # Arguments
    ///
    /// - `agent_id`: Target agent identifier
    /// - `endpoint`: Agent endpoint URL or connection string
    ///
    /// # Returns
    ///
    /// - `Ok(Transport)`: Transport ready for use
    /// - `Err(A2AError)`: Failed to get/create transport
    async fn get_transport(&self, agent_id: &str, endpoint: &str) -> A2AResult<Self::Transport> {
        self.create_transport(agent_id, endpoint, None).await
    }

    /// Remove cached transport for agent
    ///
    /// Forces creation of a new transport on next request.
    /// Useful for handling connection failures or configuration changes.
    async fn remove_transport(&self, agent_id: &str) -> A2AResult<()>;

    /// Get factory configuration
    fn get_config(&self) -> HashMap<String, Value> {
        HashMap::new()
    }
}

/// A2A Transport Builder
///
/// Builder pattern for configuring transport instances.
/// Provides a fluent interface for setting transport options.
pub struct A2ATransportBuilder {
    transport_type: String,
    endpoint: String,
    config: HashMap<String, Value>,
}

impl A2ATransportBuilder {
    /// Create a new transport builder
    pub fn new(transport_type: impl Into<String>, endpoint: impl Into<String>) -> Self {
        Self {
            transport_type: transport_type.into(),
            endpoint: endpoint.into(),
            config: HashMap::new(),
        }
    }

    /// Set a configuration option
    pub fn with_config(mut self, key: impl Into<String>, value: Value) -> Self {
        self.config.insert(key.into(), value);
        self
    }

    /// Set multiple configuration options
    pub fn with_config_map(mut self, config: HashMap<String, Value>) -> Self {
        self.config.extend(config);
        self
    }

    /// Set timeout configuration
    pub fn with_timeout(self, timeout_ms: u64) -> Self {
        self.with_config("timeout_ms", Value::from(timeout_ms))
    }

    /// Set retry configuration
    pub fn with_retries(self, max_retries: u32) -> Self {
        self.with_config("max_retries", Value::from(max_retries))
    }

    /// Set authentication configuration
    pub fn with_auth(self, auth_type: impl Into<String>, auth_value: impl Into<String>) -> Self {
        self.with_config("auth_type", Value::from(auth_type.into()))
            .with_config("auth_value", Value::from(auth_value.into()))
    }

    /// Get the transport type
    pub fn transport_type(&self) -> &str {
        &self.transport_type
    }

    /// Get the endpoint
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Get the configuration
    pub fn config(&self) -> &HashMap<String, Value> {
        &self.config
    }
}

/// Mock transport for testing
///
/// Simple transport implementation for unit tests and development.
/// Always returns successful responses with configurable behavior.
#[cfg(test)]
pub struct MockTransport {
    responses: HashMap<String, JsonRpcResponse>,
    health_status: bool,
}

#[cfg(test)]
impl MockTransport {
    /// Create a new mock transport
    pub fn new() -> Self {
        Self {
            responses: HashMap::new(),
            health_status: true,
        }
    }

    /// Configure response for a specific method
    pub fn with_response(mut self, method: String, response: JsonRpcResponse) -> Self {
        self.responses.insert(method, response);
        self
    }

    /// Set health check status
    pub fn with_health_status(mut self, healthy: bool) -> Self {
        self.health_status = healthy;
        self
    }
}

#[cfg(test)]
impl A2ATransport for MockTransport {
    async fn send_request(&self, request: JsonRpcRequest) -> A2AResult<JsonRpcResponse> {
        if let Some(response) = self.responses.get(&request.method) {
            let mut response = response.clone();
            response.id = request.id; // Match request ID
            Ok(response)
        } else {
            // Default success response
            Ok(JsonRpcResponse::success(
                request.id,
                serde_json::json!({"method": request.method, "received": true}),
            ))
        }
    }

    async fn send_notification(&self, _notification: JsonRpcNotification) -> A2AResult<()> {
        Ok(())
    }

    async fn health_check(&self) -> A2AResult<()> {
        if self.health_status {
            Ok(())
        } else {
            Err(A2AError::agent_unavailable(
                "mock-transport",
                "Health check failed",
            ))
        }
    }

    fn transport_type(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_mock_transport() {
        let transport = MockTransport::new().with_response(
            "ping".to_string(),
            JsonRpcResponse::success(json!("test-id"), json!({"pong": true})),
        );

        let request = JsonRpcRequest::new(json!("req-123"), "ping".to_string(), json!({}));
        let response = transport.send_request(request).await.unwrap();

        assert!(response.is_success());
        assert_eq!(response.id, json!("req-123"));
        assert_eq!(response.result.unwrap()["pong"], true);
    }

    #[tokio::test]
    async fn test_mock_transport_notification() {
        let transport = MockTransport::new();
        let notification = JsonRpcNotification::new("log.info".to_string(), json!({"msg": "test"}));

        let result = transport.send_notification(notification).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_transport_health_check() {
        let healthy_transport = MockTransport::new().with_health_status(true);
        assert!(healthy_transport.health_check().await.is_ok());

        let unhealthy_transport = MockTransport::new().with_health_status(false);
        assert!(unhealthy_transport.health_check().await.is_err());
    }

    #[test]
    fn test_transport_builder() {
        let builder = A2ATransportBuilder::new("http", "http://localhost:8080")
            .with_timeout(5000)
            .with_retries(3)
            .with_auth("bearer", "token123");

        assert_eq!(builder.transport_type(), "http");
        assert_eq!(builder.endpoint(), "http://localhost:8080");
        assert_eq!(builder.config()["timeout_ms"], 5000);
        assert_eq!(builder.config()["max_retries"], 3);
        assert_eq!(builder.config()["auth_type"], "bearer");
        assert_eq!(builder.config()["auth_value"], "token123");
    }

    #[tokio::test]
    async fn test_default_response() {
        let transport = MockTransport::new();
        let request =
            JsonRpcRequest::new(json!("req-456"), "unknown_method".to_string(), json!({}));

        let response = transport.send_request(request).await.unwrap();
        assert!(response.is_success());
        assert_eq!(response.result.unwrap()["method"], "unknown_method");
    }

    // NEW COMPREHENSIVE TESTS FOR 100% COVERAGE

    #[test]
    fn test_transport_builder_with_config_map() {
        let mut config_map = HashMap::new();
        config_map.insert("custom_setting".to_string(), Value::from("custom_value"));
        config_map.insert("timeout_override".to_string(), Value::from(10000));

        let builder = A2ATransportBuilder::new("websocket", "ws://localhost:9000")
            .with_config_map(config_map);

        assert_eq!(builder.transport_type(), "websocket");
        assert_eq!(builder.endpoint(), "ws://localhost:9000");
        assert_eq!(builder.config()["custom_setting"], "custom_value");
        assert_eq!(builder.config()["timeout_override"], 10000);
    }

    #[test]
    fn test_transport_builder_comprehensive_configuration() {
        let mut initial_config = HashMap::new();
        initial_config.insert("base_setting".to_string(), Value::from("base_value"));

        let builder = A2ATransportBuilder::new("grpc", "grpc://service.cluster.local:50051")
            .with_config_map(initial_config)
            .with_timeout(30000)
            .with_retries(5)
            .with_auth("mtls", "cert.pem")
            .with_config("compression", Value::from("gzip"))
            .with_config("pool_size", Value::from(20));

        let config = builder.config();
        assert_eq!(config["base_setting"], "base_value");
        assert_eq!(config["timeout_ms"], 30000);
        assert_eq!(config["max_retries"], 5);
        assert_eq!(config["auth_type"], "mtls");
        assert_eq!(config["auth_value"], "cert.pem");
        assert_eq!(config["compression"], "gzip");
        assert_eq!(config["pool_size"], 20);
    }

    #[test]
    fn test_mock_transport_type() {
        let transport = MockTransport::new();
        assert_eq!(transport.transport_type(), "mock");
    }

    #[test]
    fn test_mock_transport_metadata() {
        let transport = MockTransport::new();
        let metadata = transport.get_metadata();
        assert!(metadata.is_empty()); // Default implementation returns empty HashMap
    }

    #[tokio::test]
    async fn test_mock_transport_complex_scenarios() {
        let transport = MockTransport::new()
            .with_response(
                "complex_method".to_string(),
                JsonRpcResponse::success(
                    json!("test-id"),
                    json!({
                        "status": "success",
                        "data": {
                            "items": [1, 2, 3],
                            "total": 3
                        }
                    }),
                ),
            )
            .with_health_status(true);

        // Test complex response
        let request = JsonRpcRequest::new(
            json!("complex-req-123"),
            "complex_method".to_string(),
            json!({
                "filters": {"type": "active"},
                "pagination": {"limit": 10, "offset": 0}
            }),
        );

        let response = transport.send_request(request).await.unwrap();
        assert!(response.is_success());
        assert_eq!(response.id, json!("complex-req-123"));
        assert_eq!(response.result.unwrap()["data"]["total"], 3);

        // Test health check
        assert!(transport.health_check().await.is_ok());

        // Test notification with complex payload
        let notification = JsonRpcNotification::new(
            "system.alert".to_string(),
            json!({
                "level": "warning",
                "message": "System resource usage high",
                "details": {
                    "cpu": 85.5,
                    "memory": 92.1,
                    "timestamp": "2025-01-01T12:00:00Z"
                }
            }),
        );

        assert!(transport.send_notification(notification).await.is_ok());
    }

    // Test A2ATransportFactory default implementations
    struct MockTransportFactory;

    impl A2ATransportFactory for MockTransportFactory {
        type Transport = MockTransport;

        async fn create_transport(
            &self,
            _agent_id: &str,
            _endpoint: &str,
            _config: Option<HashMap<String, Value>>,
        ) -> A2AResult<Self::Transport> {
            Ok(MockTransport::new())
        }

        async fn remove_transport(&self, _agent_id: &str) -> A2AResult<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_transport_factory_default_implementations() {
        let factory = MockTransportFactory;

        // Test get_transport default implementation (calls create_transport)
        let transport = factory
            .get_transport("test-agent", "http://test-endpoint")
            .await
            .unwrap();
        assert_eq!(transport.transport_type(), "mock");

        // Test get_config default implementation
        let config = factory.get_config();
        assert!(config.is_empty());

        // Test remove_transport
        assert!(factory.remove_transport("test-agent").await.is_ok());
    }

    #[tokio::test]
    async fn test_transport_factory_comprehensive_workflow() {
        let factory = MockTransportFactory;

        // Create transports for multiple agents
        let transport1 = factory
            .create_transport("agent-1", "http://agent1.local", None)
            .await
            .unwrap();
        let transport2 = factory
            .create_transport(
                "agent-2",
                "http://agent2.local",
                Some({
                    let mut config = HashMap::new();
                    config.insert("timeout".to_string(), Value::from(5000));
                    config
                }),
            )
            .await
            .unwrap();

        // Test both transports
        let request1 = JsonRpcRequest::new(json!("req-1"), "ping".to_string(), json!({}));
        let request2 = JsonRpcRequest::new(json!("req-2"), "status".to_string(), json!({}));

        let response1 = transport1.send_request(request1).await.unwrap();
        let response2 = transport2.send_request(request2).await.unwrap();

        assert!(response1.is_success());
        assert!(response2.is_success());
        assert_eq!(response1.id, json!("req-1"));
        assert_eq!(response2.id, json!("req-2"));

        // Test health checks
        assert!(transport1.health_check().await.is_ok());
        assert!(transport2.health_check().await.is_ok());

        // Test get_transport with cached behavior
        let cached_transport = factory
            .get_transport("agent-1", "http://agent1.local")
            .await
            .unwrap();
        assert_eq!(cached_transport.transport_type(), "mock");

        // Test cleanup
        assert!(factory.remove_transport("agent-1").await.is_ok());
        assert!(factory.remove_transport("agent-2").await.is_ok());
    }

    #[test]
    fn test_transport_builder_edge_cases() {
        // Test with empty strings
        let builder = A2ATransportBuilder::new("", "");
        assert_eq!(builder.transport_type(), "");
        assert_eq!(builder.endpoint(), "");
        assert!(builder.config().is_empty());

        // Test with special characters and unicode
        let builder =
            A2ATransportBuilder::new("custom-transport", "proto://🌍.example.com:8080/αβγ")
                .with_config("special_chars", Value::from("!@#$%^&*()"))
                .with_config("unicode", Value::from("café ñoño αβγδε"));

        assert_eq!(builder.transport_type(), "custom-transport");
        assert_eq!(builder.endpoint(), "proto://🌍.example.com:8080/αβγ");
        assert_eq!(builder.config()["special_chars"], "!@#$%^&*()");
        assert_eq!(builder.config()["unicode"], "café ñoño αβγδε");
    }

    #[tokio::test]
    async fn test_mock_transport_error_scenarios() {
        // Test unhealthy transport error details
        let unhealthy_transport = MockTransport::new().with_health_status(false);
        let health_result = unhealthy_transport.health_check().await;

        assert!(health_result.is_err());
        let error = health_result.unwrap_err();
        let error_message = format!("{}", error);
        assert!(error_message.contains("Health check failed"));

        // Test multiple method responses
        let multi_response_transport = MockTransport::new()
            .with_response(
                "method1".to_string(),
                JsonRpcResponse::success(json!("id1"), json!({"result": "first"})),
            )
            .with_response(
                "method2".to_string(),
                JsonRpcResponse::success(json!("id2"), json!({"result": "second"})),
            );

        let req1 = JsonRpcRequest::new(json!("test1"), "method1".to_string(), json!({}));
        let req2 = JsonRpcRequest::new(json!("test2"), "method2".to_string(), json!({}));

        let resp1 = multi_response_transport.send_request(req1).await.unwrap();
        let resp2 = multi_response_transport.send_request(req2).await.unwrap();

        assert_eq!(resp1.result.unwrap()["result"], "first");
        assert_eq!(resp2.result.unwrap()["result"], "second");
        assert_eq!(resp1.id, json!("test1"));
        assert_eq!(resp2.id, json!("test2"));
    }
}
