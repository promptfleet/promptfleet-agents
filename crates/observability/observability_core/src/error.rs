//! Error types for the observability plugin system

use thiserror::Error;

/// Result type for observability operations
pub type ObservabilityResult<T> = Result<T, ObservabilityError>;

/// Comprehensive error types for observability operations
#[derive(Error, Debug, Clone)]
pub enum ObservabilityError {
    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Serialization/deserialization errors
    #[error("Serialization error: {message}")]
    Serialization { message: String },

    /// Network/transport errors
    #[error("Transport error: {message}")]
    Transport { message: String },

    /// Trace context propagation errors
    #[error("Trace context error: {message}")]
    TraceContext { message: String },

    /// Metric collection errors
    #[error("Metric error: {message}")]
    Metric { message: String },

    /// Logging errors
    #[error("Logging error: {message}")]
    Logging { message: String },

    /// Batching system errors
    #[error("Batching error: {message}")]
    Batching { message: String },

    /// Buffer overflow or memory errors
    #[error("Buffer error: {message}")]
    Buffer { message: String },

    /// Feature not enabled
    #[error("Feature not enabled: {feature}")]
    FeatureNotEnabled { feature: String },

    /// Generic errors for compatibility
    #[error("Generic error: {message}")]
    Generic { message: String },
}

impl ObservabilityError {
    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
        }
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Create a transport error
    pub fn transport(message: impl Into<String>) -> Self {
        Self::Transport {
            message: message.into(),
        }
    }

    /// Create a trace context error
    pub fn trace_context(message: impl Into<String>) -> Self {
        Self::TraceContext {
            message: message.into(),
        }
    }

    /// Create a metric error
    pub fn metric(message: impl Into<String>) -> Self {
        Self::Metric {
            message: message.into(),
        }
    }

    /// Create a logging error
    pub fn logging(message: impl Into<String>) -> Self {
        Self::Logging {
            message: message.into(),
        }
    }

    /// Create a batching error
    pub fn batching(message: impl Into<String>) -> Self {
        Self::Batching {
            message: message.into(),
        }
    }

    /// Create a buffer error
    pub fn buffer(message: impl Into<String>) -> Self {
        Self::Buffer {
            message: message.into(),
        }
    }

    /// Create a feature not enabled error
    pub fn feature_not_enabled(feature: impl Into<String>) -> Self {
        Self::FeatureNotEnabled {
            feature: feature.into(),
        }
    }

    /// Create a generic error
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic {
            message: message.into(),
        }
    }
}

#[cfg(feature = "structured-logging")]
impl From<serde_json::Error> for ObservabilityError {
    fn from(err: serde_json::Error) -> Self {
        Self::serialization(err.to_string())
    }
}
