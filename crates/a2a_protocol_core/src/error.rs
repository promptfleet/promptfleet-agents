//! A2A v1.0 Error Types

use crate::jsonrpc_error_codes;
use protocol_transport_core::JsonRpcError;
use thiserror::Error;

pub use a2a_error_codes::*;

/// A2A Protocol Error
#[derive(Error, Debug)]
pub enum A2AError {
    #[error("Method '{method}' not found in agent capabilities")]
    MethodNotFound { method: String },

    #[error("Invalid parameters for method '{method}': {details}")]
    InvalidParams { method: String, details: String },

    #[error("Agent '{agent_id}' is not available: {reason}")]
    AgentUnavailable { agent_id: String, reason: String },

    #[error("Capability validation failed: {details}")]
    CapabilityValidationFailed { details: String },

    #[error("Method '{method}' execution failed: {details}")]
    MethodExecutionFailed { method: String, details: String },

    #[error("Agent registry error: {details}")]
    RegistryError { details: String },

    #[error("Protocol validation error: {details}")]
    ProtocolValidationError { details: String },

    #[error("JSON-RPC error: {0}")]
    JsonRpcError(#[from] anyhow::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Internal A2A protocol error: {details}")]
    Internal { details: String },

    #[error("Task not found: {task_id}")]
    TaskNotFound { task_id: String },

    #[error("Task not cancelable: {task_id}")]
    TaskNotCancelable { task_id: String },

    #[error("Push notifications not supported")]
    PushNotificationNotSupported,

    #[error("Unsupported operation: {details}")]
    UnsupportedOperation { details: String },

    #[error("Content type not supported: {content_type}")]
    ContentTypeNotSupported { content_type: String },

    #[error("Invalid agent response: {details}")]
    InvalidAgentResponse { details: String },

    #[error("Version not supported: {version}")]
    VersionNotSupported { version: String },
}

/// A2A v1.0 Error Codes (spec-aligned)
pub mod a2a_error_codes {
    pub const TASK_NOT_FOUND: i64 = -32001;
    pub const TASK_NOT_CANCELABLE: i64 = -32002;
    pub const PUSH_NOTIFICATION_NOT_SUPPORTED: i64 = -32003;
    pub const UNSUPPORTED_OPERATION: i64 = -32004;
    pub const CONTENT_TYPE_NOT_SUPPORTED: i64 = -32005;
    pub const INVALID_AGENT_RESPONSE: i64 = -32006;
    pub const EXTENDED_AGENT_CARD_NOT_CONFIGURED: i64 = -32007;
    pub const EXTENSION_SUPPORT_REQUIRED: i64 = -32008;
    pub const VERSION_NOT_SUPPORTED: i64 = -32009;

    // Platform extensions (-32050 range)
    pub const AUTHENTICATION_FAILED: i64 = -32050;
    pub const AUTHORIZATION_FAILED: i64 = -32051;
    pub const RATE_LIMIT_EXCEEDED: i64 = -32052;
    pub const CAPACITY_EXCEEDED: i64 = -32053;
}

impl A2AError {
    pub fn to_jsonrpc_error(&self) -> JsonRpcError {
        match self {
            A2AError::MethodNotFound { method } => JsonRpcError::new(
                jsonrpc_error_codes::METHOD_NOT_FOUND,
                format!("Method '{}' not found", method),
            ),
            A2AError::InvalidParams { method, details } => JsonRpcError::with_data(
                jsonrpc_error_codes::INVALID_PARAMS,
                format!("Invalid parameters for method '{}'", method),
                serde_json::json!({"details": details}),
            ),
            A2AError::AgentUnavailable { agent_id, reason } => JsonRpcError::with_data(
                a2a_error_codes::TASK_NOT_FOUND,
                format!("Agent '{}' is not available", agent_id),
                serde_json::json!({"agent_id": agent_id, "reason": reason}),
            ),
            A2AError::TaskNotFound { task_id } => JsonRpcError::with_data(
                a2a_error_codes::TASK_NOT_FOUND,
                format!("Task not found: {}", task_id),
                serde_json::json!({"task_id": task_id}),
            ),
            A2AError::TaskNotCancelable { task_id } => JsonRpcError::with_data(
                a2a_error_codes::TASK_NOT_CANCELABLE,
                format!("Task not cancelable: {}", task_id),
                serde_json::json!({"task_id": task_id}),
            ),
            A2AError::PushNotificationNotSupported => JsonRpcError::new(
                a2a_error_codes::PUSH_NOTIFICATION_NOT_SUPPORTED,
                "Push notifications not supported".to_string(),
            ),
            A2AError::UnsupportedOperation { details } => JsonRpcError::with_data(
                a2a_error_codes::UNSUPPORTED_OPERATION,
                "Unsupported operation".to_string(),
                serde_json::json!({"details": details}),
            ),
            A2AError::ContentTypeNotSupported { content_type } => JsonRpcError::with_data(
                a2a_error_codes::CONTENT_TYPE_NOT_SUPPORTED,
                "Content type not supported".to_string(),
                serde_json::json!({"content_type": content_type}),
            ),
            A2AError::InvalidAgentResponse { details } => JsonRpcError::with_data(
                a2a_error_codes::INVALID_AGENT_RESPONSE,
                "Invalid agent response".to_string(),
                serde_json::json!({"details": details}),
            ),
            A2AError::VersionNotSupported { version } => JsonRpcError::with_data(
                a2a_error_codes::VERSION_NOT_SUPPORTED,
                format!("Version not supported: {}", version),
                serde_json::json!({"version": version}),
            ),
            A2AError::CapabilityValidationFailed { details } => JsonRpcError::with_data(
                a2a_error_codes::PUSH_NOTIFICATION_NOT_SUPPORTED,
                "Capability validation failed".to_string(),
                serde_json::json!({"details": details}),
            ),
            A2AError::MethodExecutionFailed { method, details } => JsonRpcError::with_data(
                a2a_error_codes::UNSUPPORTED_OPERATION,
                format!("Method '{}' execution failed", method),
                serde_json::json!({"method": method, "details": details}),
            ),
            A2AError::RegistryError { details } => JsonRpcError::with_data(
                a2a_error_codes::CONTENT_TYPE_NOT_SUPPORTED,
                "Agent registry error".to_string(),
                serde_json::json!({"details": details}),
            ),
            A2AError::ProtocolValidationError { details } => JsonRpcError::with_data(
                a2a_error_codes::INVALID_AGENT_RESPONSE,
                "Protocol validation error".to_string(),
                serde_json::json!({"details": details}),
            ),
            A2AError::JsonRpcError(err) => JsonRpcError::new(
                jsonrpc_error_codes::INTERNAL_ERROR,
                format!("JSON-RPC error: {}", err),
            ),
            A2AError::SerializationError(err) => JsonRpcError::new(
                jsonrpc_error_codes::INTERNAL_ERROR,
                format!("Serialization error: {}", err),
            ),
            A2AError::Internal { details } => JsonRpcError::with_data(
                jsonrpc_error_codes::INTERNAL_ERROR,
                "Internal A2A protocol error".to_string(),
                serde_json::json!({"details": details}),
            ),
        }
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::MethodNotFound {
            method: method.into(),
        }
    }
    pub fn invalid_params(method: impl Into<String>, details: impl Into<String>) -> Self {
        Self::InvalidParams {
            method: method.into(),
            details: details.into(),
        }
    }
    pub fn agent_unavailable(agent_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::AgentUnavailable {
            agent_id: agent_id.into(),
            reason: reason.into(),
        }
    }
    pub fn capability_validation_failed(details: impl Into<String>) -> Self {
        Self::CapabilityValidationFailed {
            details: details.into(),
        }
    }
    pub fn method_execution_failed(method: impl Into<String>, details: impl Into<String>) -> Self {
        Self::MethodExecutionFailed {
            method: method.into(),
            details: details.into(),
        }
    }
    pub fn registry_error(details: impl Into<String>) -> Self {
        Self::RegistryError {
            details: details.into(),
        }
    }
    pub fn protocol_validation_error(details: impl Into<String>) -> Self {
        Self::ProtocolValidationError {
            details: details.into(),
        }
    }
    pub fn internal(details: impl Into<String>) -> Self {
        Self::Internal {
            details: details.into(),
        }
    }
    pub fn task_not_found(task_id: impl Into<String>) -> Self {
        Self::TaskNotFound {
            task_id: task_id.into(),
        }
    }
    pub fn task_not_cancelable(task_id: impl Into<String>) -> Self {
        Self::TaskNotCancelable {
            task_id: task_id.into(),
        }
    }
    pub fn unsupported_operation(details: impl Into<String>) -> Self {
        Self::UnsupportedOperation {
            details: details.into(),
        }
    }
    pub fn version_not_supported(version: impl Into<String>) -> Self {
        Self::VersionNotSupported {
            version: version.into(),
        }
    }
}

pub type A2AResult<T> = Result<T, A2AError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_not_found_error() {
        let error = A2AError::task_not_found("task-123");
        let rpc = error.to_jsonrpc_error();
        assert_eq!(rpc.code, a2a_error_codes::TASK_NOT_FOUND);
    }

    #[test]
    fn test_method_not_found_error() {
        let error = A2AError::method_not_found("unknown_method");
        let rpc = error.to_jsonrpc_error();
        assert_eq!(rpc.code, jsonrpc_error_codes::METHOD_NOT_FOUND);
    }

    #[test]
    fn test_internal_error() {
        let error = A2AError::internal("Database connection failed");
        let rpc = error.to_jsonrpc_error();
        assert_eq!(rpc.code, jsonrpc_error_codes::INTERNAL_ERROR);
    }
}
