//! Error types for enhanced structured logging

use thiserror::Error;

/// Result type for structured logging operations
pub type Result<T> = std::result::Result<T, StructuredLoggingError>;

/// Enhanced error types for structured logging operations
#[derive(Error, Debug, Clone)]
pub enum StructuredLoggingError {
    /// Wraps observability_core errors
    #[error("Observability error: {0}")]
    Observability(#[from] observability_core::ObservabilityError),

    /// Performance optimization errors
    #[error("Performance optimization error: {message}")]
    Performance { message: String },

    /// String interning errors
    #[error("String interning error: {message}")]
    StringInterning { message: String },

    /// Buffer pool errors
    #[error("Buffer pool error: {message}")]
    BufferPool { message: String },

    /// Correlation enhancement errors
    #[error("Correlation error: {message}")]
    Correlation { message: String },

    /// Baggage management errors
    #[error("Baggage error: {message}")]
    Baggage { message: String },

    /// Scoped context errors
    #[error("Scoped context error: {message}")]
    ScopedContext { message: String },

    /// Convenience API errors
    #[error("Convenience API error: {message}")]
    Convenience { message: String },

    /// Configuration errors specific to enhanced features
    #[error("Enhanced configuration error: {message}")]
    EnhancedConfig { message: String },

    /// Fast path errors
    #[error("Fast path error: {message}")]
    FastPath { message: String },

    /// Feature not enabled
    #[error("Feature not enabled: {feature}. Enable with cargo feature '{feature}'")]
    FeatureNotEnabled { feature: String },
}

impl StructuredLoggingError {
    /// Create a performance error
    pub fn performance<T: Into<String>>(message: T) -> Self {
        Self::Performance {
            message: message.into(),
        }
    }

    /// Create a string interning error
    pub fn string_interning<T: Into<String>>(message: T) -> Self {
        Self::StringInterning {
            message: message.into(),
        }
    }

    /// Create a buffer pool error
    pub fn buffer_pool<T: Into<String>>(message: T) -> Self {
        Self::BufferPool {
            message: message.into(),
        }
    }

    /// Create a correlation error
    pub fn correlation<T: Into<String>>(message: T) -> Self {
        Self::Correlation {
            message: message.into(),
        }
    }

    /// Create a baggage error
    pub fn baggage<T: Into<String>>(message: T) -> Self {
        Self::Baggage {
            message: message.into(),
        }
    }

    /// Create a scoped context error
    pub fn scoped_context<T: Into<String>>(message: T) -> Self {
        Self::ScopedContext {
            message: message.into(),
        }
    }

    /// Create a convenience API error
    pub fn convenience<T: Into<String>>(message: T) -> Self {
        Self::Convenience {
            message: message.into(),
        }
    }

    /// Create an enhanced configuration error
    pub fn enhanced_config<T: Into<String>>(message: T) -> Self {
        Self::EnhancedConfig {
            message: message.into(),
        }
    }

    /// Create a fast path error
    pub fn fast_path<T: Into<String>>(message: T) -> Self {
        Self::FastPath {
            message: message.into(),
        }
    }

    /// Create a feature not enabled error
    pub fn feature_not_enabled<T: Into<String>>(feature: T) -> Self {
        Self::FeatureNotEnabled {
            feature: feature.into(),
        }
    }
}

impl From<serde_json::Error> for StructuredLoggingError {
    fn from(err: serde_json::Error) -> Self {
        Self::performance(format!("JSON serialization error: {}", err))
    }
}

impl From<std::io::Error> for StructuredLoggingError {
    fn from(err: std::io::Error) -> Self {
        Self::performance(format!("IO error: {}", err))
    }
}
