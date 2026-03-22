//! Common error types for all protocols

use thiserror::Error;

/// **Universal Transport Error**
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Timeout error: {0}")]
    Timeout(String),

    #[error("HTTP error {status}: {message}")]
    Http {
        status: u16,
        message: String,
        /// Optional raw response body (captured for richer diagnostics)
        body: Option<Vec<u8>>,
        /// Optional response headers (useful for MCP tool-error payloads or broker metadata)
        headers: Option<std::collections::HashMap<String, String>>,
    },

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Authentication error: {0}")]
    Authentication(String),
}

/// **Universal Protocol Error**  
#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("Unsupported protocol: {0}")]
    UnsupportedProtocol(String),

    #[error("Protocol not found: {0}")]
    ProtocolNotFound(String),

    #[error("Protocol validation error: {0}")]
    Validation(String),

    #[error("Protocol parsing error: {0}")]
    Parsing(String),

    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// **Result types**
pub type TransportResult<T> = Result<T, TransportError>;
pub type ProtocolResult<T> = Result<T, ProtocolError>;

impl TransportError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TransportError::Network(_)
                | TransportError::Timeout(_)
                | TransportError::Http {
                    status: 500..=599,
                    ..
                }
        )
    }

    /// Check if error is due to authentication
    pub fn is_auth_error(&self) -> bool {
        matches!(
            self,
            TransportError::Authentication(_)
                | TransportError::Http {
                    status: 401 | 403,
                    ..
                }
        )
    }
}

impl ProtocolError {
    /// Create internal error
    pub fn internal_error(msg: &str) -> Self {
        Self::Internal(msg.to_string())
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            ProtocolError::Transport(te) => te.is_retryable(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_transport_error_variants() {
        let network_error = TransportError::Network("Connection failed".to_string());
        assert_eq!(
            network_error.to_string(),
            "Network error: Connection failed"
        );
        assert!(network_error.is_retryable());
        assert!(!network_error.is_auth_error());

        let serialization_error = TransportError::Serialization("Invalid JSON".to_string());
        assert_eq!(
            serialization_error.to_string(),
            "Serialization error: Invalid JSON"
        );
        assert!(!serialization_error.is_retryable());
        assert!(!serialization_error.is_auth_error());

        let timeout_error = TransportError::Timeout("Request timed out".to_string());
        assert_eq!(
            timeout_error.to_string(),
            "Timeout error: Request timed out"
        );
        assert!(timeout_error.is_retryable());
        assert!(!timeout_error.is_auth_error());

        let config_error = TransportError::Configuration("Invalid config".to_string());
        assert_eq!(
            config_error.to_string(),
            "Configuration error: Invalid config"
        );
        assert!(!config_error.is_retryable());
        assert!(!config_error.is_auth_error());

        let auth_error = TransportError::Authentication("Invalid credentials".to_string());
        assert_eq!(
            auth_error.to_string(),
            "Authentication error: Invalid credentials"
        );
        assert!(!auth_error.is_retryable());
        assert!(auth_error.is_auth_error());
    }

    #[test]
    fn test_transport_http_error() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());

        let http_error = TransportError::Http {
            status: 404,
            message: "Not Found".to_string(),
            body: Some(b"Resource not found".to_vec()),
            headers: Some(headers),
        };

        assert_eq!(http_error.to_string(), "HTTP error 404: Not Found");
        assert!(!http_error.is_retryable());
        assert!(!http_error.is_auth_error());

        // Test auth errors
        let auth_error_401 = TransportError::Http {
            status: 401,
            message: "Unauthorized".to_string(),
            body: None,
            headers: None,
        };
        assert!(auth_error_401.is_auth_error());

        let auth_error_403 = TransportError::Http {
            status: 403,
            message: "Forbidden".to_string(),
            body: None,
            headers: None,
        };
        assert!(auth_error_403.is_auth_error());

        // Test retryable server errors
        let server_error = TransportError::Http {
            status: 503,
            message: "Service Unavailable".to_string(),
            body: None,
            headers: None,
        };
        assert!(server_error.is_retryable());

        // Test non-retryable client errors
        let client_error = TransportError::Http {
            status: 400,
            message: "Bad Request".to_string(),
            body: None,
            headers: None,
        };
        assert!(!client_error.is_retryable());
    }

    #[test]
    fn test_protocol_error_variants() {
        let unsupported_error = ProtocolError::UnsupportedProtocol("XYZ".to_string());
        assert_eq!(unsupported_error.to_string(), "Unsupported protocol: XYZ");
        assert!(!unsupported_error.is_retryable());

        let not_found_error = ProtocolError::ProtocolNotFound("ABC".to_string());
        assert_eq!(not_found_error.to_string(), "Protocol not found: ABC");
        assert!(!not_found_error.is_retryable());

        let validation_error = ProtocolError::Validation("Invalid params".to_string());
        assert_eq!(
            validation_error.to_string(),
            "Protocol validation error: Invalid params"
        );
        assert!(!validation_error.is_retryable());

        let parsing_error = ProtocolError::Parsing("Malformed request".to_string());
        assert_eq!(
            parsing_error.to_string(),
            "Protocol parsing error: Malformed request"
        );
        assert!(!parsing_error.is_retryable());

        let internal_error = ProtocolError::Internal("Something went wrong".to_string());
        assert_eq!(
            internal_error.to_string(),
            "Internal error: Something went wrong"
        );
        assert!(!internal_error.is_retryable());
    }

    #[test]
    fn test_protocol_error_from_transport_error() {
        let transport_error = TransportError::Network("Connection lost".to_string());
        let protocol_error: ProtocolError = transport_error.into();

        match protocol_error {
            ProtocolError::Transport(te) => {
                assert!(te.is_retryable());
                assert_eq!(te.to_string(), "Network error: Connection lost");
            }
            _ => panic!("Expected Transport variant"),
        }
    }

    #[test]
    fn test_protocol_error_from_serde_error() {
        let json_str = r#"{"invalid": }"#;
        let serde_error = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
        let protocol_error: ProtocolError = serde_error.into();

        match protocol_error {
            ProtocolError::Serialization(_) => {
                assert!(!protocol_error.is_retryable());
            }
            _ => panic!("Expected Serialization variant"),
        }
    }

    #[test]
    fn test_protocol_error_internal_helper() {
        let error = ProtocolError::internal_error("Test internal error");
        assert_eq!(error.to_string(), "Internal error: Test internal error");
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_protocol_error_retryable_transport() {
        let retryable_transport = TransportError::Timeout("Request timeout".to_string());
        let protocol_error = ProtocolError::Transport(retryable_transport);
        assert!(protocol_error.is_retryable());

        let non_retryable_transport = TransportError::Configuration("Bad config".to_string());
        let protocol_error = ProtocolError::Transport(non_retryable_transport);
        assert!(!protocol_error.is_retryable());
    }

    #[test]
    fn test_result_types() {
        let transport_result: TransportResult<String> = Ok("success".to_string());
        assert!(transport_result.is_ok());

        let transport_error_result: TransportResult<String> =
            Err(TransportError::Network("fail".to_string()));
        assert!(transport_error_result.is_err());

        let protocol_result: ProtocolResult<String> = Ok("success".to_string());
        assert!(protocol_result.is_ok());

        let protocol_error_result: ProtocolResult<String> =
            Err(ProtocolError::Validation("fail".to_string()));
        assert!(protocol_error_result.is_err());
    }

    #[test]
    fn test_error_debug_format() {
        let transport_error = TransportError::Network("test".to_string());
        let debug_str = format!("{:?}", transport_error);
        assert!(debug_str.contains("Network"));
        assert!(debug_str.contains("test"));

        let protocol_error = ProtocolError::UnsupportedProtocol("test".to_string());
        let debug_str = format!("{:?}", protocol_error);
        assert!(debug_str.contains("UnsupportedProtocol"));
        assert!(debug_str.contains("test"));
    }
}
