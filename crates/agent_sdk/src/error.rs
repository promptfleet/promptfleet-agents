//! Error types for the Agent SDK
//!
//! This module provides unified error handling for the SDK, wrapping
//! underlying A2A protocol errors and adding SDK-specific error types.

use a2a_protocol_core::error::A2AError;
use thiserror::Error;

/// SDK-specific error types
///
/// Wraps underlying A2A protocol errors and adds SDK-specific error cases
/// for better error handling and user experience.
#[derive(Error, Debug)]
pub enum SdkError {
    /// A2A protocol error
    #[error("A2A protocol error: {0}")]
    A2AProtocol(#[from] A2AError),

    /// Configuration error
    #[error("Configuration error: {details}")]
    Configuration { details: String },

    /// Agent initialization error
    #[error("Agent initialization failed: {reason}")]
    AgentInitialization { reason: String },

    /// Skill registration error
    #[error("Failed to register skill '{skill}': {reason}")]
    SkillRegistration { skill: String, reason: String },

    /// Server startup error
    #[cfg(feature = "a2a-server")]
    #[error("Server startup failed: {reason}")]
    ServerStartup { reason: String },

    /// Client connection error
    #[cfg(feature = "a2a-client")]
    #[error("Client connection failed: {endpoint} - {reason}")]
    ClientConnection { endpoint: String, reason: String },

    /// Method execution error
    #[error("Method execution failed: {method} - {details}")]
    MethodExecution { method: String, details: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error
    #[error("SDK error: {0}")]
    Generic(#[from] anyhow::Error),

    /// Feature not enabled
    #[error("Feature '{feature}' is not enabled. Enable it in Cargo.toml")]
    FeatureNotEnabled { feature: String },

    /// Invalid input
    #[error("Invalid input: {details}")]
    InvalidInput { details: String },
}

impl SdkError {
    /// Create a configuration error
    pub fn configuration(details: impl Into<String>) -> Self {
        Self::Configuration {
            details: details.into(),
        }
    }

    /// Create an agent initialization error
    pub fn agent_initialization(reason: impl Into<String>) -> Self {
        Self::AgentInitialization {
            reason: reason.into(),
        }
    }

    /// Create a skill registration error
    pub fn skill_registration(skill: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::SkillRegistration {
            skill: skill.into(),
            reason: reason.into(),
        }
    }

    /// Create a server startup error
    #[cfg(feature = "a2a-server")]
    pub fn server_startup(reason: impl Into<String>) -> Self {
        Self::ServerStartup {
            reason: reason.into(),
        }
    }

    /// Create a client connection error
    #[cfg(feature = "a2a-client")]
    pub fn client_connection(endpoint: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ClientConnection {
            endpoint: endpoint.into(),
            reason: reason.into(),
        }
    }

    /// Create a method execution error
    pub fn method_execution(method: impl Into<String>, details: impl Into<String>) -> Self {
        Self::MethodExecution {
            method: method.into(),
            details: details.into(),
        }
    }

    /// Create a feature not enabled error
    pub fn feature_not_enabled(feature: impl Into<String>) -> Self {
        Self::FeatureNotEnabled {
            feature: feature.into(),
        }
    }

    /// Create an invalid input error
    pub fn invalid_input(details: impl Into<String>) -> Self {
        Self::InvalidInput {
            details: details.into(),
        }
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            SdkError::A2AProtocol(a2a_err) => {
                // Some A2A errors are recoverable (temporary failures)
                matches!(
                    a2a_err,
                    A2AError::AgentUnavailable { .. } | A2AError::Internal { .. }
                )
            }
            #[cfg(feature = "a2a-client")]
            SdkError::ClientConnection { .. } => true,
            SdkError::MethodExecution { .. } => true,
            SdkError::Io(_) => true,
            _ => false,
        }
    }

    /// Get error category for monitoring/logging
    pub fn category(&self) -> &'static str {
        match self {
            SdkError::A2AProtocol(_) => "a2a_protocol",
            SdkError::Configuration { .. } => "configuration",
            SdkError::AgentInitialization { .. } => "initialization",
            SdkError::SkillRegistration { .. } => "skill",
            #[cfg(feature = "a2a-server")]
            SdkError::ServerStartup { .. } => "server",
            #[cfg(feature = "a2a-client")]
            SdkError::ClientConnection { .. } => "client",
            SdkError::MethodExecution { .. } => "method_execution",
            SdkError::Serialization(_) => "serialization",
            SdkError::Io(_) => "io",
            SdkError::Generic(_) => "generic",
            SdkError::FeatureNotEnabled { .. } => "feature",
            SdkError::InvalidInput { .. } => "input_validation",
        }
    }
}

impl From<SdkError> for A2AError {
    fn from(error: SdkError) -> Self {
        match error {
            SdkError::A2AProtocol(err) => err,
            SdkError::InvalidInput { details } => A2AError::invalid_params("agent_sdk", details),
            SdkError::MethodExecution { method, details } => {
                A2AError::method_execution_failed(method, details)
            }
            SdkError::Serialization(err) => A2AError::SerializationError(err),
            SdkError::Generic(err) => A2AError::JsonRpcError(err),
            SdkError::FeatureNotEnabled { feature } => {
                A2AError::unsupported_operation(format!("Feature not enabled: {}", feature))
            }
            SdkError::Configuration { details } => A2AError::internal(details),
            SdkError::AgentInitialization { reason } => A2AError::internal(reason),
            SdkError::SkillRegistration { reason, .. } => A2AError::internal(reason),
            SdkError::Io(err) => A2AError::internal(err.to_string()),
            #[cfg(feature = "a2a-server")]
            SdkError::ServerStartup { reason } => A2AError::internal(reason),
            #[cfg(feature = "a2a-client")]
            SdkError::ClientConnection { endpoint, reason } => {
                A2AError::agent_unavailable(endpoint, reason)
            }
        }
    }
}

/// Result type for SDK operations
pub type SdkResult<T> = Result<T, SdkError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let config_err = SdkError::configuration("Invalid port");
        assert_eq!(config_err.category(), "configuration");
        assert!(!config_err.is_recoverable());

        let init_err = SdkError::agent_initialization("Missing name");
        assert_eq!(init_err.category(), "initialization");

        let skill_err = SdkError::skill_registration("test_method", "Invalid parameters");
        assert_eq!(skill_err.category(), "skill");
    }

    #[test]
    fn test_error_recoverability() {
        let a2a_unavailable =
            SdkError::A2AProtocol(A2AError::agent_unavailable("test-agent", "Network timeout"));
        assert!(a2a_unavailable.is_recoverable());

        let feature_err = SdkError::feature_not_enabled("server");
        assert!(!feature_err.is_recoverable());
    }

    #[test]
    fn test_error_categories() {
        let config_err = SdkError::configuration("test");
        assert_eq!(config_err.category(), "configuration");

        let a2a_err = SdkError::A2AProtocol(A2AError::internal("test"));
        assert_eq!(a2a_err.category(), "a2a_protocol");
    }

    #[test]
    fn test_error_formatting() {
        let err = SdkError::skill_registration("get_weather", "Missing handler");
        let formatted = format!("{}", err);
        assert!(formatted.contains("get_weather"));
        assert!(formatted.contains("Missing handler"));
    }

    #[cfg(feature = "a2a-client")]
    #[test]
    fn test_client_connection_error() {
        let err = SdkError::client_connection("http://example.com", "Connection refused");
        assert_eq!(err.category(), "client");
        assert!(err.is_recoverable());

        let formatted = format!("{}", err);
        assert!(formatted.contains("http://example.com"));
        assert!(formatted.contains("Connection refused"));
    }

    #[cfg(feature = "a2a-server")]
    #[test]
    fn test_server_startup_error() {
        let err = SdkError::server_startup("Port already in use");
        assert_eq!(err.category(), "server");
        assert!(!err.is_recoverable());
    }
}
