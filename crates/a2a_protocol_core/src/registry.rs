//! A2A Method Registry
//!
//! Manages registration and lookup of A2A protocol methods.
//! This module provides the core registry functionality without
//! any transport or infrastructure dependencies.

use crate::{A2AError, A2AResult, AgentCard};
use protocol_transport_core::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// A2A Method Handler function type
///
/// Handlers receive a JSON-RPC request and return a JSON-RPC response.
/// All A2A protocol logic is handled through JSON-RPC 2.0 messages.
pub type A2AMethodHandler = Arc<dyn Fn(JsonRpcRequest) -> A2AResult<JsonRpcResponse> + Send + Sync>;

/// A2A Notification Handler function type
///
/// Notification handlers receive notifications and perform side effects
/// without returning responses (fire-and-forget).
pub type A2ANotificationHandler = Arc<dyn Fn(JsonRpcNotification) -> A2AResult<()> + Send + Sync>;

/// Method metadata
///
/// Contains information about a registered method including
/// its description, parameters, and capabilities.
#[derive(Debug, Clone)]
pub struct MethodMetadata {
    pub name: String,
    pub description: String,
    pub parameters: Option<Value>,
    pub returns: Option<Value>,
    pub is_notification: bool,
}

/// A2A Method Registry
///
/// Central registry for A2A protocol methods and their handlers.
/// Provides method registration, lookup, and validation functionality
/// without any transport dependencies.
///
/// # Design Principles
///
/// - **Pure Domain Logic**: No transport or infrastructure concerns
/// - **JSON-RPC Foundation**: All methods use JSON-RPC 2.0 message types
/// - **Thread Safe**: Can be safely shared across multiple threads
/// - **Method Metadata**: Rich metadata for discovery and documentation
///
/// # Examples
///
/// ```rust
/// use a2a_protocol_core::{A2AMethodRegistry, JsonRpcRequest, JsonRpcResponse, A2AError};
/// use serde_json::json;
/// use std::sync::Arc;
///
/// fn example() -> Result<(), A2AError> {
///     let mut registry = A2AMethodRegistry::new();
///     
///     // Register a method handler
///     registry.register_method(
///         "ping",
///         "Simple ping method",
///         Arc::new(|request| {
///             Ok(JsonRpcResponse::success(request.id, json!({"pong": true})))
///         })
///     );
///     
///     // Handle requests
///     let request = JsonRpcRequest::new(json!("req-123"), "ping".to_string(), json!({}));
///     let response = registry.handle_request(request)?;
///     Ok(())
/// }
/// ```
pub struct A2AMethodRegistry {
    /// Method handlers (request/response)
    method_handlers: HashMap<String, A2AMethodHandler>,
    /// Notification handlers (fire-and-forget)
    notification_handlers: HashMap<String, A2ANotificationHandler>,
    /// Method metadata
    method_metadata: HashMap<String, MethodMetadata>,
    /// Agent card information
    agent_card: Option<AgentCard>,
}

impl A2AMethodRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            method_handlers: HashMap::new(),
            notification_handlers: HashMap::new(),
            method_metadata: HashMap::new(),
            agent_card: None,
        }
    }

    /// Create a registry with agent card
    pub fn with_agent_card(agent_card: AgentCard) -> Self {
        let mut registry = Self::new();
        registry.set_agent_card(agent_card);
        registry
    }

    /// Set the agent card for this registry
    pub fn set_agent_card(&mut self, agent_card: AgentCard) {
        self.agent_card = Some(agent_card);
    }

    /// Get the agent card
    pub fn agent_card(&self) -> Option<&AgentCard> {
        self.agent_card.as_ref()
    }

    /// Register a method handler
    ///
    /// Registers a handler function for JSON-RPC requests to the specified method.
    ///
    /// # Arguments
    ///
    /// - `method`: Method name (e.g., "ping", "process_data")
    /// - `description`: Human-readable description of the method
    /// - `handler`: Function that handles requests for this method
    pub fn register_method(
        &mut self,
        method: impl Into<String>,
        description: impl Into<String>,
        handler: A2AMethodHandler,
    ) {
        let method = method.into();
        let description = description.into();

        self.method_handlers.insert(method.clone(), handler);
        self.method_metadata.insert(
            method.clone(),
            MethodMetadata {
                name: method,
                description,
                parameters: None,
                returns: None,
                is_notification: false,
            },
        );
    }

    /// Register a method handler with metadata
    ///
    /// Registers a method handler with detailed parameter and return type information.
    pub fn register_method_with_metadata(
        &mut self,
        method: impl Into<String>,
        description: impl Into<String>,
        parameters: Option<Value>,
        returns: Option<Value>,
        handler: A2AMethodHandler,
    ) {
        let method = method.into();
        let description = description.into();

        self.method_handlers.insert(method.clone(), handler);
        self.method_metadata.insert(
            method.clone(),
            MethodMetadata {
                name: method,
                description,
                parameters,
                returns,
                is_notification: false,
            },
        );
    }

    /// Register a notification handler
    ///
    /// Registers a handler function for JSON-RPC notifications (fire-and-forget).
    ///
    /// # Arguments
    ///
    /// - `method`: Method name (e.g., "log.info", "metric.counter")
    /// - `description`: Human-readable description of the notification
    /// - `handler`: Function that handles notifications for this method
    pub fn register_notification(
        &mut self,
        method: impl Into<String>,
        description: impl Into<String>,
        handler: A2ANotificationHandler,
    ) {
        let method = method.into();
        let description = description.into();

        self.notification_handlers.insert(method.clone(), handler);
        self.method_metadata.insert(
            method.clone(),
            MethodMetadata {
                name: method,
                description,
                parameters: None,
                returns: None,
                is_notification: true,
            },
        );
    }

    /// Handle a JSON-RPC request
    ///
    /// Looks up the method handler and executes it with the request.
    ///
    /// # Arguments
    ///
    /// - `request`: The JSON-RPC 2.0 request to handle
    ///
    /// # Returns
    ///
    /// - `Ok(JsonRpcResponse)`: Success response from method handler
    /// - `Err(A2AError)`: Method not found or handler error
    pub fn handle_request(&self, request: JsonRpcRequest) -> A2AResult<JsonRpcResponse> {
        // Validate request
        if !request.is_valid() {
            return Err(A2AError::protocol_validation_error(
                "Invalid JSON-RPC request",
            ));
        }

        // Look up handler
        let handler = self
            .method_handlers
            .get(&request.method)
            .ok_or_else(|| A2AError::method_not_found(&request.method))?;

        // Execute handler
        let method = request.method.clone();
        handler(request).map_err(|e| A2AError::method_execution_failed(&method, e.to_string()))
    }

    /// Handle a JSON-RPC notification
    ///
    /// Looks up the notification handler and executes it.
    ///
    /// # Arguments
    ///
    /// - `notification`: The JSON-RPC 2.0 notification to handle
    ///
    /// # Returns
    ///
    /// - `Ok(())`: Notification handled successfully
    /// - `Err(A2AError)`: Method not found or handler error
    pub fn handle_notification(&self, notification: JsonRpcNotification) -> A2AResult<()> {
        // Validate notification
        if !notification.is_valid() {
            return Err(A2AError::protocol_validation_error(
                "Invalid JSON-RPC notification",
            ));
        }

        // Look up handler
        let handler = self
            .notification_handlers
            .get(&notification.method)
            .ok_or_else(|| A2AError::method_not_found(&notification.method))?;

        // Execute handler
        let method = notification.method.clone();
        handler(notification).map_err(|e| A2AError::method_execution_failed(&method, e.to_string()))
    }

    /// Check if method is registered
    pub fn has_method(&self, method: &str) -> bool {
        self.method_handlers.contains_key(method)
    }

    /// Check if notification is registered
    pub fn has_notification(&self, method: &str) -> bool {
        self.notification_handlers.contains_key(method)
    }

    /// Get method metadata
    pub fn get_method_metadata(&self, method: &str) -> Option<&MethodMetadata> {
        self.method_metadata.get(method)
    }

    /// List all registered methods
    pub fn list_methods(&self) -> Vec<String> {
        self.method_handlers.keys().cloned().collect()
    }

    /// List all registered notifications
    pub fn list_notifications(&self) -> Vec<String> {
        self.notification_handlers.keys().cloned().collect()
    }

    /// Get all method metadata
    pub fn get_all_metadata(&self) -> &HashMap<String, MethodMetadata> {
        &self.method_metadata
    }

    /// Unregister a method
    pub fn unregister_method(&mut self, method: &str) -> bool {
        let removed_handler = self.method_handlers.remove(method).is_some();
        self.method_metadata.remove(method);
        removed_handler
    }

    /// Unregister a notification
    pub fn unregister_notification(&mut self, method: &str) -> bool {
        let removed_handler = self.notification_handlers.remove(method).is_some();
        self.method_metadata.remove(method);
        removed_handler
    }

    /// Clear all registrations
    pub fn clear(&mut self) {
        self.method_handlers.clear();
        self.notification_handlers.clear();
        self.method_metadata.clear();
    }

    /// Get registry statistics
    pub fn stats(&self) -> RegistryStats {
        RegistryStats {
            method_count: self.method_handlers.len(),
            notification_count: self.notification_handlers.len(),
            total_registrations: self.method_metadata.len(),
        }
    }
}

impl Default for A2AMethodRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry statistics
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub method_count: usize,
    pub notification_count: usize,
    pub total_registrations: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_method_registration() {
        let mut registry = A2AMethodRegistry::new();

        // Register a method
        registry.register_method(
            "ping",
            "Simple ping method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({"pong": true})))),
        );

        assert!(registry.has_method("ping"));
        assert!(!registry.has_method("unknown"));
        assert_eq!(registry.list_methods(), vec!["ping"]);

        let metadata = registry.get_method_metadata("ping").unwrap();
        assert_eq!(metadata.name, "ping");
        assert_eq!(metadata.description, "Simple ping method");
        assert!(!metadata.is_notification);
    }

    #[test]
    fn test_notification_registration() {
        let mut registry = A2AMethodRegistry::new();

        // Register a notification
        registry.register_notification(
            "log.info",
            "Info log notification",
            Arc::new(|_notification| Ok(())),
        );

        assert!(registry.has_notification("log.info"));
        assert!(!registry.has_notification("unknown"));
        assert_eq!(registry.list_notifications(), vec!["log.info"]);

        let metadata = registry.get_method_metadata("log.info").unwrap();
        assert_eq!(metadata.name, "log.info");
        assert_eq!(metadata.description, "Info log notification");
        assert!(metadata.is_notification);
    }

    #[test]
    fn test_request_handling() {
        let mut registry = A2AMethodRegistry::new();

        // Register ping method
        registry.register_method(
            "ping",
            "Simple ping method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({"pong": true})))),
        );

        // Test successful request
        let request = JsonRpcRequest::new(json!("req-123"), "ping".to_string(), json!({}));
        let response = registry.handle_request(request).unwrap();

        assert!(response.is_success());
        assert_eq!(response.id, json!("req-123"));
        assert_eq!(response.result.unwrap()["pong"], true);

        // Test unknown method
        let unknown_request =
            JsonRpcRequest::new(json!("req-456"), "unknown".to_string(), json!({}));
        let result = registry.handle_request(unknown_request);
        assert!(result.is_err());
        if let Err(A2AError::MethodNotFound { method }) = result {
            assert_eq!(method, "unknown");
        } else {
            panic!("Expected MethodNotFound error");
        }
    }

    #[test]
    fn test_notification_handling() {
        let mut registry = A2AMethodRegistry::new();

        // Register log notification
        registry.register_notification(
            "log.info",
            "Info log notification",
            Arc::new(|_notification| Ok(())),
        );

        // Test successful notification
        let notification = JsonRpcNotification::new("log.info".to_string(), json!({"msg": "test"}));
        let result = registry.handle_notification(notification);
        assert!(result.is_ok());

        // Test unknown notification
        let unknown_notification = JsonRpcNotification::new("unknown".to_string(), json!({}));
        let result = registry.handle_notification(unknown_notification);
        assert!(result.is_err());
    }

    #[test]
    fn test_registry_management() {
        let mut registry = A2AMethodRegistry::new();

        // Register methods
        registry.register_method(
            "ping",
            "Simple ping",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({})))),
        );
        registry.register_notification("log", "Logger", Arc::new(|_| Ok(())));

        let stats = registry.stats();
        assert_eq!(stats.method_count, 1);
        assert_eq!(stats.notification_count, 1);
        assert_eq!(stats.total_registrations, 2);

        // Unregister
        assert!(registry.unregister_method("ping"));
        assert!(!registry.unregister_method("unknown"));

        let stats = registry.stats();
        assert_eq!(stats.method_count, 0);
        assert_eq!(stats.notification_count, 1);
        assert_eq!(stats.total_registrations, 1);

        // Clear all
        registry.clear();
        let stats = registry.stats();
        assert_eq!(stats.total_registrations, 0);
    }

    #[test]
    fn test_method_with_metadata() {
        let mut registry = A2AMethodRegistry::new();

        registry.register_method_with_metadata(
            "calculate",
            "Calculate something",
            Some(json!({"type": "object", "properties": {"a": {"type": "number"}}})),
            Some(json!({"type": "number"})),
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!(42)))),
        );

        let metadata = registry.get_method_metadata("calculate").unwrap();
        assert!(metadata.parameters.is_some());
        assert!(metadata.returns.is_some());
        assert!(!metadata.is_notification);
    }

    // NEW TESTS TO IMPROVE COVERAGE

    #[test]
    fn test_with_agent_card_constructor() {
        let agent_card =
            AgentCard::new("test-agent".to_string()).with_capability("test", "Test capability");

        let registry = A2AMethodRegistry::with_agent_card(agent_card.clone());

        assert!(registry.agent_card().is_some());
        assert_eq!(registry.agent_card().unwrap().name, "test-agent");
    }

    #[test]
    fn test_agent_card_getter() {
        let mut registry = A2AMethodRegistry::new();

        // Initially no agent card
        assert!(registry.agent_card().is_none());

        // Set agent card
        let agent_card = AgentCard::new("test-agent".to_string());
        registry.set_agent_card(agent_card.clone());

        // Now should have agent card
        assert!(registry.agent_card().is_some());
        assert_eq!(registry.agent_card().unwrap().name, "test-agent");
    }

    #[test]
    fn test_get_all_metadata() {
        let mut registry = A2AMethodRegistry::new();

        registry.register_method(
            "method1",
            "First method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({})))),
        );

        registry.register_notification("notif1", "First notification", Arc::new(|_| Ok(())));

        let all_metadata = registry.get_all_metadata();
        assert_eq!(all_metadata.len(), 2);
        assert!(all_metadata.contains_key("method1"));
        assert!(all_metadata.contains_key("notif1"));

        // Check notification flag
        assert!(!all_metadata["method1"].is_notification);
        assert!(all_metadata["notif1"].is_notification);
    }

    #[test]
    fn test_unregister_notification() {
        let mut registry = A2AMethodRegistry::new();

        registry.register_notification("test_notif", "Test notification", Arc::new(|_| Ok(())));

        assert!(registry.has_notification("test_notif"));

        // Unregister existing notification
        assert!(registry.unregister_notification("test_notif"));
        assert!(!registry.has_notification("test_notif"));

        // Unregister non-existent notification
        assert!(!registry.unregister_notification("unknown_notif"));
    }

    #[test]
    fn test_default_impl() {
        let registry = A2AMethodRegistry::default();

        assert_eq!(registry.list_methods().len(), 0);
        assert_eq!(registry.list_notifications().len(), 0);
        assert!(registry.agent_card().is_none());
    }

    #[test]
    fn test_invalid_request_validation() {
        let mut registry = A2AMethodRegistry::new();

        // Register a method
        registry.register_method(
            "test",
            "Test method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({})))),
        );

        // Create invalid request with empty method
        let mut invalid_request =
            JsonRpcRequest::new(json!("req-1"), "test".to_string(), json!({}));
        invalid_request.method = "".to_string(); // Make it invalid

        let result = registry.handle_request(invalid_request);
        assert!(result.is_err());

        match result {
            Err(A2AError::ProtocolValidationError { .. }) => {
                // Expected error type
            }
            _ => panic!("Expected ProtocolValidationError"),
        }
    }

    #[test]
    fn test_invalid_notification_validation() {
        let mut registry = A2AMethodRegistry::new();

        // Register a notification
        registry.register_notification("test", "Test notification", Arc::new(|_| Ok(())));

        // Create invalid notification with empty method
        let mut invalid_notification = JsonRpcNotification::new("test".to_string(), json!({}));
        invalid_notification.method = "".to_string(); // Make it invalid

        let result = registry.handle_notification(invalid_notification);
        assert!(result.is_err());

        match result {
            Err(A2AError::ProtocolValidationError { .. }) => {
                // Expected error type
            }
            _ => panic!("Expected ProtocolValidationError"),
        }
    }

    #[test]
    fn test_method_execution_error_handling() {
        let mut registry = A2AMethodRegistry::new();

        // Register method that returns an error
        registry.register_method(
            "failing_method",
            "Method that fails",
            Arc::new(|_| Err(A2AError::internal("Test method error"))),
        );

        let request = JsonRpcRequest::new(json!("req-1"), "failing_method".to_string(), json!({}));
        let result = registry.handle_request(request);

        assert!(result.is_err());
        match result {
            Err(A2AError::MethodExecutionFailed { method, .. }) => {
                assert_eq!(method, "failing_method");
            }
            _ => panic!("Expected MethodExecutionFailed error"),
        }
    }

    #[test]
    fn test_notification_execution_error_handling() {
        let mut registry = A2AMethodRegistry::new();

        // Register notification that returns an error
        registry.register_notification(
            "failing_notification",
            "Notification that fails",
            Arc::new(|_| Err(A2AError::internal("Test notification error"))),
        );

        let notification = JsonRpcNotification::new("failing_notification".to_string(), json!({}));
        let result = registry.handle_notification(notification);

        assert!(result.is_err());
        match result {
            Err(A2AError::MethodExecutionFailed { method, .. }) => {
                assert_eq!(method, "failing_notification");
            }
            _ => panic!("Expected MethodExecutionFailed error"),
        }
    }

    #[test]
    fn test_method_handler_closure_execution() {
        let mut registry = A2AMethodRegistry::new();

        // Register method and actually call it
        registry.register_method(
            "echo",
            "Echo method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, request.params))),
        );

        let request =
            JsonRpcRequest::new(json!("req-1"), "echo".to_string(), json!({"data": "test"}));
        let response = registry.handle_request(request).unwrap();

        assert!(response.is_success());
        assert_eq!(response.result.unwrap()["data"], "test");
    }

    #[test]
    fn test_notification_handler_closure_execution() {
        let mut registry = A2AMethodRegistry::new();

        // Register notification and actually call it
        registry.register_notification(
            "log",
            "Log notification",
            Arc::new(|_| {
                // Simulate successful processing
                Ok(())
            }),
        );

        let notification = JsonRpcNotification::new("log".to_string(), json!({"message": "test"}));
        let result = registry.handle_notification(notification);

        assert!(result.is_ok());
    }

    #[test]
    fn test_comprehensive_registry_operations() {
        let mut registry = A2AMethodRegistry::new();

        // Set agent card
        let agent_card = AgentCard::new("comprehensive-test".to_string())
            .with_capability("method1", "First method")
            .with_capability("method2", "Second method");

        registry.set_agent_card(agent_card);

        // Register multiple methods and notifications
        registry.register_method(
            "method1",
            "First method",
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({"result": 1})))),
        );

        registry.register_method_with_metadata(
            "method2",
            "Second method with metadata",
            Some(json!({"type": "object"})),
            Some(json!({"type": "number"})),
            Arc::new(|request| Ok(JsonRpcResponse::success(request.id, json!({"result": 2})))),
        );

        registry.register_notification("notif1", "First notification", Arc::new(|_| Ok(())));
        registry.register_notification("notif2", "Second notification", Arc::new(|_| Ok(())));

        // Test comprehensive stats
        let stats = registry.stats();
        assert_eq!(stats.method_count, 2);
        assert_eq!(stats.notification_count, 2);
        assert_eq!(stats.total_registrations, 4);

        // Test method listing
        let methods = registry.list_methods();
        assert_eq!(methods.len(), 2);
        assert!(methods.contains(&"method1".to_string()));
        assert!(methods.contains(&"method2".to_string()));

        // Test notification listing
        let notifications = registry.list_notifications();
        assert_eq!(notifications.len(), 2);
        assert!(notifications.contains(&"notif1".to_string()));
        assert!(notifications.contains(&"notif2".to_string()));

        // Test metadata access
        let metadata = registry.get_method_metadata("method2").unwrap();
        assert!(metadata.parameters.is_some());
        assert!(metadata.returns.is_some());

        // Test method execution
        let request = JsonRpcRequest::new(json!("req-1"), "method1".to_string(), json!({}));
        let response = registry.handle_request(request).unwrap();
        assert_eq!(response.result.unwrap()["result"], 1);

        // Test notification execution
        let notification = JsonRpcNotification::new("notif1".to_string(), json!({}));
        let result = registry.handle_notification(notification);
        assert!(result.is_ok());

        // Test selective unregistration
        assert!(registry.unregister_method("method1"));
        assert_eq!(registry.list_methods().len(), 1);

        assert!(registry.unregister_notification("notif1"));
        assert_eq!(registry.list_notifications().len(), 1);

        // Final stats check
        let final_stats = registry.stats();
        assert_eq!(final_stats.method_count, 1);
        assert_eq!(final_stats.notification_count, 1);
        assert_eq!(final_stats.total_registrations, 2);
    }
}
