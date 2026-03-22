//! Pure JSON-RPC 2.0 Protocol Implementation
//!
//! This module provides a pure implementation of JSON-RPC 2.0 types and structures
//! optimized for WebAssembly environments. It follows the JSON-RPC 2.0 specification
//! exactly without any agent-specific extensions.
//!
//! # JSON-RPC 2.0 Overview
//!
//! JSON-RPC 2.0 is a stateless, light-weight remote procedure call (RPC) protocol.
//! It defines two types of messages:
//!
//! ## Request Messages
//! Have an `id` field and expect a response:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": "request-123",
//!   "method": "ping",
//!   "params": {"timestamp": "2024-01-01T00:00:00Z"}
//! }
//! ```
//!
//! ## Notification Messages  
//! No `id` field, fire-and-forget:
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "log.info",
//!   "params": {"message": "Task completed", "level": "info"}
//! }
//! ```

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 request ID type
///
/// Can be a string, number, or null according to the JSON-RPC 2.0 specification.
/// The ID is used to correlate requests with responses and must be echoed back
/// in the response exactly as received.
///
/// # Valid ID Types
///
/// - **String**: `"request-123"`, `"abc-def-456"`
/// - **Number**: `1`, `42`, `1234567890`
/// - **Null**: `null` (though discouraged for traceability)
pub type JsonRpcId = serde_json::Value;

/// Incoming JSON-RPC message types
///
/// Represents all possible incoming JSON-RPC 2.0 messages that can be received
/// and parsed. The enum uses serde's untagged feature to automatically detect
/// the message type based on the presence of the `id` field.
///
/// # Message Type Detection
///
/// - **Request**: Has `id` field - expects a response
/// - **Notification**: No `id` field - fire-and-forget
/// - **Batch**: Array of requests/notifications (optional feature)
#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum JsonRpcIncoming {
    /// Standard JSON-RPC 2.0 request with ID
    ///
    /// Contains an `id` field and expects a response. Used for business logic
    /// operations where the caller needs to know the result.
    Request(JsonRpcRequest),
    /// JSON-RPC 2.0 notification (no ID, no response expected)
    ///
    /// Fire-and-forget message with no `id` field. Used for operations
    /// where the caller doesn't need to know the result.
    Notification(JsonRpcNotification),
    /// Batch request (array of requests/notifications)
    ///
    /// Allows multiple requests/notifications to be sent in a single message.
    #[cfg(feature = "batch")]
    Batch(Vec<JsonRpcIncoming>),
}

/// JSON-RPC 2.0 Request structure
///
/// Represents a JSON-RPC 2.0 request message that expects a response.
/// All requests must include the `jsonrpc`, `id`, and `method` fields.
/// The `params` field is optional and defaults to null.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (must be "2.0")
    ///
    /// This field identifies the version of the JSON-RPC protocol.
    /// It must always be exactly "2.0" for compliance.
    pub jsonrpc: String,
    /// Request ID (string, number, or null)
    ///
    /// Unique identifier for this request that will be echoed back
    /// in the response to correlate request and response pairs.
    pub id: JsonRpcId,
    /// Method name to invoke
    ///
    /// The name of the method to be invoked on the server.
    /// Method names are case-sensitive strings.
    pub method: String,
    /// Method parameters (optional, defaults to null)
    ///
    /// Parameters to be passed to the method. Can be any JSON value
    /// including objects, arrays, primitives, or null.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 Notification structure
///
/// Represents a JSON-RPC 2.0 notification message that does not expect a response.
/// Notifications are "fire-and-forget" messages used for operations where the
/// caller doesn't need to know the result.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct JsonRpcNotification {
    /// JSON-RPC version (must be "2.0")
    ///
    /// This field identifies the version of the JSON-RPC protocol.
    /// It must always be exactly "2.0" for compliance.
    pub jsonrpc: String,
    /// Method name to invoke
    ///
    /// The name of the method to be invoked on the server.
    /// Method names are case-sensitive strings.
    pub method: String,
    /// Method parameters (optional, defaults to null)
    ///
    /// Parameters to be passed to the method. Can be any JSON value
    /// including objects, arrays, primitives, or null.
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 Response structure
///
/// Represents a JSON-RPC 2.0 response message sent back to the client.
/// A response must contain either a `result` (for success) or an `error`
/// (for failure), but never both. The `id` field must match the request ID.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    ///
    /// This field identifies the version of the JSON-RPC protocol.
    /// It must always be exactly "2.0" for compliance.
    pub jsonrpc: String,
    /// Successful result (mutually exclusive with error)
    ///
    /// Present only for successful responses. Contains the actual
    /// result data returned by the method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error object (mutually exclusive with result)
    ///
    /// Present only for error responses. Contains detailed error
    /// information including code, message, and optional data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// Request ID (copied from request)
    ///
    /// Must exactly match the ID from the original request to
    /// enable proper correlation of requests and responses.
    pub id: JsonRpcId,
}

/// JSON-RPC 2.0 Error Object structure
///
/// Represents detailed error information in JSON-RPC 2.0 error responses.
/// Includes a standard error code, human-readable message, and optional
/// additional data for debugging and error handling.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    /// Error code (standard or application-defined)
    ///
    /// Numeric code identifying the type of error. Standard JSON-RPC 2.0
    /// codes are in the range -32700 to -32600, with server errors in
    /// the range -32099 to -32000.
    pub code: i64,
    /// Error message
    ///
    /// Human-readable description of the error. Should be clear and
    /// helpful for debugging and user feedback.
    pub message: String,
    /// Optional additional error data
    ///
    /// Additional information about the error, such as stack traces,
    /// error context, or debugging information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Standard JSON-RPC 2.0 Error Codes
///
/// This module defines the standard error codes specified in the JSON-RPC 2.0
/// specification. These codes ensure consistent error handling across all implementations.
///
/// # Standard JSON-RPC 2.0 Codes
///
/// The official JSON-RPC 2.0 specification defines these error codes:
///
/// | Code | Name | Description |
/// |------|------|-------------|
/// | -32700 | Parse Error | Invalid JSON was received |
/// | -32600 | Invalid Request | JSON is not a valid request object |
/// | -32601 | Method Not Found | Method does not exist |
/// | -32602 | Invalid Params | Invalid method parameters |
/// | -32603 | Internal Error | Internal JSON-RPC error |
/// | -32099 to -32000 | Server Error | Reserved for implementation-defined errors |
pub mod error_codes {
    /// Parse error - Invalid JSON was received by the server
    ///
    /// This error occurs when the received data cannot be parsed as valid JSON.
    /// It indicates a client-side error in JSON formatting or encoding.
    ///
    /// **Code**: `-32700`
    pub const PARSE_ERROR: i64 = -32700;

    /// Invalid Request - The JSON sent is not a valid Request object
    ///
    /// This error occurs when the JSON is valid but doesn't conform to the
    /// JSON-RPC 2.0 request specification (missing required fields, etc.).
    ///
    /// **Code**: `-32600`
    pub const INVALID_REQUEST: i64 = -32600;

    /// Method not found - The method does not exist / is not available
    ///
    /// This error occurs when the requested method is not registered or
    /// available on the server. The method name may be misspelled or
    /// the method may not be implemented.
    ///
    /// **Code**: `-32601`
    pub const METHOD_NOT_FOUND: i64 = -32601;

    /// Invalid params - Invalid method parameter(s)
    ///
    /// This error occurs when the method exists but the provided parameters
    /// are invalid (wrong types, missing required params, validation failures).
    ///
    /// **Code**: `-32602`
    pub const INVALID_PARAMS: i64 = -32602;

    /// Internal error - Internal JSON-RPC error
    ///
    /// This error occurs when an unexpected error happens during method
    /// execution. It indicates a server-side problem rather than a client error.
    ///
    /// **Code**: `-32603`
    pub const INTERNAL_ERROR: i64 = -32603;
}

// Implementation methods for convenience
impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request
    pub fn new(id: JsonRpcId, method: String, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method,
            params,
        }
    }

    /// Check if this is a valid JSON-RPC 2.0 request
    pub fn is_valid(&self) -> bool {
        self.jsonrpc == JSONRPC_VERSION && !self.method.is_empty()
    }
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC 2.0 notification
    pub fn new(method: String, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
        }
    }

    /// Check if this is a valid JSON-RPC 2.0 notification
    pub fn is_valid(&self) -> bool {
        self.jsonrpc == JSONRPC_VERSION && !self.method.is_empty()
    }
}

impl JsonRpcResponse {
    /// Create a successful JSON-RPC 2.0 response
    pub fn success(id: JsonRpcId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error JSON-RPC 2.0 response
    pub fn error(id: JsonRpcId, code: i64, message: String) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(JsonRpcError::new(code, message)),
            id,
        }
    }

    /// Create an error JSON-RPC 2.0 response with additional data
    pub fn error_with_data(
        id: JsonRpcId,
        code: i64,
        message: String,
        data: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(JsonRpcError::with_data(code, message, data)),
            id,
        }
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        self.result.is_some() && self.error.is_none()
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.error.is_some() && self.result.is_none()
    }
}

impl JsonRpcError {
    /// Create a new JSON-RPC 2.0 error
    pub fn new(code: i64, message: String) -> Self {
        Self {
            code,
            message,
            data: None,
        }
    }

    /// Create a new JSON-RPC 2.0 error with additional data
    pub fn with_data(code: i64, message: String, data: serde_json::Value) -> Self {
        Self {
            code,
            message,
            data: Some(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_creation() {
        let request = JsonRpcRequest::new(
            json!("test-123"),
            "ping".to_string(),
            json!({"timestamp": "2024-01-01T00:00:00Z"}),
        );

        assert_eq!(request.jsonrpc, JSONRPC_VERSION);
        assert_eq!(request.id, json!("test-123"));
        assert_eq!(request.method, "ping");
        assert!(request.is_valid());
    }

    #[test]
    fn test_notification_creation() {
        let notification = JsonRpcNotification::new(
            "log.info".to_string(),
            json!({"message": "test", "level": "info"}),
        );

        assert_eq!(notification.jsonrpc, JSONRPC_VERSION);
        assert_eq!(notification.method, "log.info");
        assert!(notification.is_valid());
    }

    #[test]
    fn test_success_response() {
        let response = JsonRpcResponse::success(
            json!("req-123"),
            json!({"pong": true, "timestamp": "2024-01-01T00:00:00Z"}),
        );

        assert!(response.is_success());
        assert!(!response.is_error());
        assert_eq!(response.id, json!("req-123"));
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_error_response() {
        let response = JsonRpcResponse::error(
            json!("req-456"),
            error_codes::METHOD_NOT_FOUND,
            "Method not found".to_string(),
        );

        assert!(response.is_error());
        assert!(!response.is_success());
        assert_eq!(response.id, json!("req-456"));
        assert!(response.error.is_some());
        assert!(response.result.is_none());

        let error = response.error.unwrap();
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "Method not found");
    }

    #[test]
    fn test_error_with_data() {
        let response = JsonRpcResponse::error_with_data(
            json!("req-789"),
            error_codes::INVALID_PARAMS,
            "Invalid parameters".to_string(),
            json!({"expected": "number", "received": "string"}),
        );

        assert!(response.is_error());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32602);
        assert!(error.data.is_some());
        assert_eq!(error.data.unwrap()["expected"], "number");
    }

    #[test]
    fn test_incoming_message_parsing() {
        // Test request parsing
        let request_json = json!({
            "jsonrpc": JSONRPC_VERSION,
            "id": "req-123",
            "method": "ping",
            "params": {}
        });

        let incoming: JsonRpcIncoming = serde_json::from_value(request_json).unwrap();
        match incoming {
            JsonRpcIncoming::Request(req) => {
                assert_eq!(req.method, "ping");
                assert_eq!(req.id, json!("req-123"));
            }
            _ => panic!("Should be a request"),
        }

        // Test notification parsing
        let notification_json = json!({
            "jsonrpc": JSONRPC_VERSION,
            "method": "log.info",
            "params": {"message": "test"}
        });

        let incoming: JsonRpcIncoming = serde_json::from_value(notification_json).unwrap();
        match incoming {
            JsonRpcIncoming::Notification(notif) => {
                assert_eq!(notif.method, "log.info");
            }
            _ => panic!("Should be a notification"),
        }
    }
}
