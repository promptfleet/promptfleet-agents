//! Common header management for all protocols

use std::collections::HashMap;

/// **Common Protocol Headers**
pub struct ProtocolHeaders {
    pub protocol: String,
    pub version: String,
    pub correlation_id: String,
    pub client_agent_id: Option<String>,
    pub trace_id: Option<String>,
}

impl ProtocolHeaders {
    /// Extract protocol headers from raw headers
    pub fn from_headers(headers: &HashMap<String, String>) -> Self {
        Self {
            protocol: headers
                .get("x-protocol")
                .cloned()
                .unwrap_or_else(|| "UNKNOWN".to_string()),
            version: headers
                .get("x-protocol-version")
                .cloned()
                .unwrap_or_else(|| "1.0".to_string()),
            correlation_id: headers
                .get("x-correlation-id")
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            client_agent_id: headers.get("x-client-agent-id").cloned(),
            trace_id: headers.get("x-trace-id").cloned(),
        }
    }

    /// Convert to raw headers HashMap
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("x-protocol".to_string(), self.protocol.clone());
        headers.insert("x-protocol-version".to_string(), self.version.clone());
        headers.insert("x-correlation-id".to_string(), self.correlation_id.clone());

        if let Some(client_id) = &self.client_agent_id {
            headers.insert("x-client-agent-id".to_string(), client_id.clone());
        }

        if let Some(trace_id) = &self.trace_id {
            headers.insert("x-trace-id".to_string(), trace_id.clone());
        }

        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_from_headers_with_all_values() {
        let mut headers = HashMap::new();
        headers.insert("x-protocol".to_string(), "A2A".to_string());
        headers.insert("x-protocol-version".to_string(), "2024".to_string());
        headers.insert(
            "x-correlation-id".to_string(),
            "test-correlation-123".to_string(),
        );
        headers.insert("x-client-agent-id".to_string(), "agent-456".to_string());
        headers.insert("x-trace-id".to_string(), "trace-789".to_string());

        let protocol_headers = ProtocolHeaders::from_headers(&headers);

        assert_eq!(protocol_headers.protocol, "A2A");
        assert_eq!(protocol_headers.version, "2024");
        assert_eq!(protocol_headers.correlation_id, "test-correlation-123");
        assert_eq!(
            protocol_headers.client_agent_id,
            Some("agent-456".to_string())
        );
        assert_eq!(protocol_headers.trace_id, Some("trace-789".to_string()));
    }

    #[test]
    fn test_from_headers_with_partial_values() {
        let mut headers = HashMap::new();
        headers.insert("x-protocol".to_string(), "MCP".to_string());
        headers.insert("x-correlation-id".to_string(), "partial-test".to_string());
        // Missing version, client_agent_id, and trace_id

        let protocol_headers = ProtocolHeaders::from_headers(&headers);

        assert_eq!(protocol_headers.protocol, "MCP");
        assert_eq!(protocol_headers.version, "1.0"); // Default value
        assert_eq!(protocol_headers.correlation_id, "partial-test");
        assert_eq!(protocol_headers.client_agent_id, None);
        assert_eq!(protocol_headers.trace_id, None);
    }

    #[test]
    fn test_from_headers_with_empty_map() {
        let headers = HashMap::new();
        let protocol_headers = ProtocolHeaders::from_headers(&headers);

        assert_eq!(protocol_headers.protocol, "UNKNOWN"); // Default value
        assert_eq!(protocol_headers.version, "1.0"); // Default value
                                                     // correlation_id should be a UUID v4, so just check it's not empty
        assert!(!protocol_headers.correlation_id.is_empty());
        assert!(protocol_headers.correlation_id.contains('-')); // UUIDs have dashes
        assert_eq!(protocol_headers.client_agent_id, None);
        assert_eq!(protocol_headers.trace_id, None);
    }

    #[test]
    fn test_from_headers_with_only_protocol() {
        let mut headers = HashMap::new();
        headers.insert("x-protocol".to_string(), "REST".to_string());

        let protocol_headers = ProtocolHeaders::from_headers(&headers);

        assert_eq!(protocol_headers.protocol, "REST");
        assert_eq!(protocol_headers.version, "1.0"); // Default
        assert!(!protocol_headers.correlation_id.is_empty()); // Generated UUID
        assert_eq!(protocol_headers.client_agent_id, None);
        assert_eq!(protocol_headers.trace_id, None);
    }

    #[test]
    fn test_to_headers_with_all_values() {
        let protocol_headers = ProtocolHeaders {
            protocol: "A2A".to_string(),
            version: "2024".to_string(),
            correlation_id: "test-correlation-123".to_string(),
            client_agent_id: Some("agent-456".to_string()),
            trace_id: Some("trace-789".to_string()),
        };

        let headers = protocol_headers.to_headers();

        assert_eq!(headers.get("x-protocol"), Some(&"A2A".to_string()));
        assert_eq!(headers.get("x-protocol-version"), Some(&"2024".to_string()));
        assert_eq!(
            headers.get("x-correlation-id"),
            Some(&"test-correlation-123".to_string())
        );
        assert_eq!(
            headers.get("x-client-agent-id"),
            Some(&"agent-456".to_string())
        );
        assert_eq!(headers.get("x-trace-id"), Some(&"trace-789".to_string()));
        assert_eq!(headers.len(), 5);
    }

    #[test]
    fn test_to_headers_with_minimal_values() {
        let protocol_headers = ProtocolHeaders {
            protocol: "MCP".to_string(),
            version: "1.0".to_string(),
            correlation_id: "minimal-test".to_string(),
            client_agent_id: None,
            trace_id: None,
        };

        let headers = protocol_headers.to_headers();

        assert_eq!(headers.get("x-protocol"), Some(&"MCP".to_string()));
        assert_eq!(headers.get("x-protocol-version"), Some(&"1.0".to_string()));
        assert_eq!(
            headers.get("x-correlation-id"),
            Some(&"minimal-test".to_string())
        );
        assert_eq!(headers.get("x-client-agent-id"), None);
        assert_eq!(headers.get("x-trace-id"), None);
        assert_eq!(headers.len(), 3); // Only the required headers
    }

    #[test]
    fn test_to_headers_with_only_client_agent_id() {
        let protocol_headers = ProtocolHeaders {
            protocol: "REST".to_string(),
            version: "1.1".to_string(),
            correlation_id: "client-only-test".to_string(),
            client_agent_id: Some("only-client".to_string()),
            trace_id: None,
        };

        let headers = protocol_headers.to_headers();

        assert_eq!(headers.get("x-protocol"), Some(&"REST".to_string()));
        assert_eq!(headers.get("x-protocol-version"), Some(&"1.1".to_string()));
        assert_eq!(
            headers.get("x-correlation-id"),
            Some(&"client-only-test".to_string())
        );
        assert_eq!(
            headers.get("x-client-agent-id"),
            Some(&"only-client".to_string())
        );
        assert_eq!(headers.get("x-trace-id"), None);
        assert_eq!(headers.len(), 4);
    }

    #[test]
    fn test_to_headers_with_only_trace_id() {
        let protocol_headers = ProtocolHeaders {
            protocol: "PUBSUB".to_string(),
            version: "1.0".to_string(),
            correlation_id: "trace-only-test".to_string(),
            client_agent_id: None,
            trace_id: Some("only-trace".to_string()),
        };

        let headers = protocol_headers.to_headers();

        assert_eq!(headers.get("x-protocol"), Some(&"PUBSUB".to_string()));
        assert_eq!(headers.get("x-protocol-version"), Some(&"1.0".to_string()));
        assert_eq!(
            headers.get("x-correlation-id"),
            Some(&"trace-only-test".to_string())
        );
        assert_eq!(headers.get("x-client-agent-id"), None);
        assert_eq!(headers.get("x-trace-id"), Some(&"only-trace".to_string()));
        assert_eq!(headers.len(), 4);
    }

    #[test]
    fn test_round_trip_conversion() {
        // Test that from_headers -> to_headers preserves all data
        let mut original_headers = HashMap::new();
        original_headers.insert("x-protocol".to_string(), "A2A".to_string());
        original_headers.insert("x-protocol-version".to_string(), "2024".to_string());
        original_headers.insert(
            "x-correlation-id".to_string(),
            "round-trip-test".to_string(),
        );
        original_headers.insert(
            "x-client-agent-id".to_string(),
            "round-trip-agent".to_string(),
        );
        original_headers.insert("x-trace-id".to_string(), "round-trip-trace".to_string());

        let protocol_headers = ProtocolHeaders::from_headers(&original_headers);
        let converted_headers = protocol_headers.to_headers();

        // All protocol headers should match
        assert_eq!(
            converted_headers.get("x-protocol"),
            original_headers.get("x-protocol")
        );
        assert_eq!(
            converted_headers.get("x-protocol-version"),
            original_headers.get("x-protocol-version")
        );
        assert_eq!(
            converted_headers.get("x-correlation-id"),
            original_headers.get("x-correlation-id")
        );
        assert_eq!(
            converted_headers.get("x-client-agent-id"),
            original_headers.get("x-client-agent-id")
        );
        assert_eq!(
            converted_headers.get("x-trace-id"),
            original_headers.get("x-trace-id")
        );
    }

    #[test]
    fn test_round_trip_conversion_with_extra_headers() {
        // Test that from_headers ignores non-protocol headers, but essential data is preserved
        let mut headers_with_extras = HashMap::new();
        headers_with_extras.insert("x-protocol".to_string(), "MCP".to_string());
        headers_with_extras.insert("x-correlation-id".to_string(), "extra-test".to_string());
        headers_with_extras.insert("content-type".to_string(), "application/json".to_string()); // Extra header
        headers_with_extras.insert("authorization".to_string(), "Bearer token".to_string()); // Extra header

        let protocol_headers = ProtocolHeaders::from_headers(&headers_with_extras);
        let converted_headers = protocol_headers.to_headers();

        // Protocol headers should be preserved
        assert_eq!(
            converted_headers.get("x-protocol"),
            Some(&"MCP".to_string())
        );
        assert_eq!(
            converted_headers.get("x-correlation-id"),
            Some(&"extra-test".to_string())
        );

        // Extra headers should not be in the converted headers
        assert_eq!(converted_headers.get("content-type"), None);
        assert_eq!(converted_headers.get("authorization"), None);
    }

    #[test]
    fn test_default_values_consistency() {
        // Test that default values are consistent across different calls
        let empty_headers = HashMap::new();
        let headers1 = ProtocolHeaders::from_headers(&empty_headers);
        let headers2 = ProtocolHeaders::from_headers(&empty_headers);

        // Protocol and version should be consistent defaults
        assert_eq!(headers1.protocol, headers2.protocol);
        assert_eq!(headers1.version, headers2.version);

        // Correlation IDs should be different (generated UUIDs)
        assert_ne!(headers1.correlation_id, headers2.correlation_id);

        // Optional fields should be consistent (None)
        assert_eq!(headers1.client_agent_id, headers2.client_agent_id);
        assert_eq!(headers1.trace_id, headers2.trace_id);
    }
}
