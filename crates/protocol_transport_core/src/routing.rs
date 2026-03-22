//! Common routing utilities

use crate::{ProtocolError, UniversalRequest, UniversalResponse};
use std::collections::HashMap;

/// **Protocol Router** - Routes requests to appropriate protocol handlers
pub struct ProtocolRouter {
    handlers: HashMap<String, Box<dyn crate::AsyncProtocolHandler>>,
}

impl ProtocolRouter {
    /// Create new protocol router
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a protocol handler
    pub fn register<H>(&mut self, protocol: &str, handler: H)
    where
        H: crate::AsyncProtocolHandler + 'static,
    {
        self.handlers
            .insert(protocol.to_string(), Box::new(handler));
    }

    /// Route request to appropriate handler
    pub fn route(&self, request: UniversalRequest) -> Result<UniversalResponse, ProtocolError> {
        let protocol = &request.protocol;

        match self.handlers.get(protocol) {
            Some(handler) => handler.handle_request_sync(request),
            None => Err(ProtocolError::UnsupportedProtocol(protocol.clone())),
        }
    }
}

impl Default for ProtocolRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// **Route Pattern**
#[derive(Debug, Clone)]
pub struct Route {
    pub pattern: String,
    pub protocol: String,
    pub methods: Vec<String>,
}

/// **Route Matcher**
pub struct RouteMatcher {
    routes: Vec<Route>,
}

impl RouteMatcher {
    /// Create new route matcher
    pub fn new() -> Self {
        Self { routes: Vec::new() }
    }

    /// Add route pattern
    pub fn add_route(&mut self, pattern: String, protocol: String, methods: Vec<String>) {
        self.routes.push(Route {
            pattern,
            protocol,
            methods,
        });
    }

    /// Match path to protocol
    pub fn match_path(&self, path: &str, method: &str) -> Option<String> {
        for route in &self.routes {
            if self.pattern_matches(&route.pattern, path)
                && route.methods.iter().any(|m| m.eq_ignore_ascii_case(method))
            {
                return Some(route.protocol.clone());
            }
        }
        None
    }

    /// Simple pattern matching (can be enhanced with regex)
    fn pattern_matches(&self, pattern: &str, path: &str) -> bool {
        if pattern.ends_with("/*") {
            let prefix = &pattern[..pattern.len() - 2];
            path.starts_with(prefix)
        } else {
            pattern == path
        }
    }
}

impl Default for RouteMatcher {
    fn default() -> Self {
        let mut matcher = Self::new();

        // Add default routes
        matcher.add_route(
            "/jsonrpc".to_string(),
            "A2A".to_string(),
            vec!["POST".to_string()],
        );
        matcher.add_route(
            "/a2a/*".to_string(),
            "A2A".to_string(),
            vec!["POST".to_string()],
        );
        matcher.add_route(
            "/mcp/*".to_string(),
            "MCP".to_string(),
            vec!["POST".to_string()],
        );
        matcher.add_route(
            "/rest/*".to_string(),
            "REST".to_string(),
            vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
            ],
        );
        matcher.add_route(
            "/pubsub/*".to_string(),
            "PUBSUB".to_string(),
            vec!["POST".to_string()],
        );

        matcher
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // Mock handler for testing
    struct MockHandler {
        protocol_name: String,
        should_error: bool,
    }

    impl MockHandler {
        fn new(protocol_name: &str) -> Self {
            Self {
                protocol_name: protocol_name.to_string(),
                should_error: false,
            }
        }

        fn new_with_error(protocol_name: &str) -> Self {
            Self {
                protocol_name: protocol_name.to_string(),
                should_error: true,
            }
        }
    }

    impl crate::AsyncProtocolHandler for MockHandler {
        fn protocol_name(&self) -> &'static str {
            // Need to return a static str, so we'll use a known protocol name
            "TEST"
        }

        fn handle_request_sync(
            &self,
            request: UniversalRequest,
        ) -> Result<UniversalResponse, ProtocolError> {
            if self.should_error {
                return Err(ProtocolError::Internal("Mock error".to_string()));
            }

            Ok(UniversalResponse {
                status: 200,
                headers: HashMap::new(),
                body: format!("Handled by {}", self.protocol_name).into_bytes(),
                protocol: request.protocol,
                correlation_id: request.correlation_id,
            })
        }
    }

    fn create_test_request(protocol: &str) -> UniversalRequest {
        UniversalRequest {
            method: "POST".to_string(),
            uri: "/test".to_string(),
            headers: HashMap::new(),
            body: b"test body".to_vec(),
            protocol: protocol.to_string(),
            correlation_id: "test-correlation".to_string(),
        }
    }

    #[test]
    fn test_protocol_router_new() {
        let router = ProtocolRouter::new();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_protocol_router_default() {
        let router = ProtocolRouter::default();
        assert!(router.handlers.is_empty());
    }

    #[test]
    fn test_protocol_router_register_handler() {
        let mut router = ProtocolRouter::new();
        let handler = MockHandler::new("A2A");

        router.register("A2A", handler);
        assert_eq!(router.handlers.len(), 1);
        assert!(router.handlers.contains_key("A2A"));
    }

    #[test]
    fn test_protocol_router_register_multiple_handlers() {
        let mut router = ProtocolRouter::new();

        router.register("A2A", MockHandler::new("A2A"));
        router.register("MCP", MockHandler::new("MCP"));
        router.register("REST", MockHandler::new("REST"));

        assert_eq!(router.handlers.len(), 3);
        assert!(router.handlers.contains_key("A2A"));
        assert!(router.handlers.contains_key("MCP"));
        assert!(router.handlers.contains_key("REST"));
    }

    #[test]
    fn test_protocol_router_route_success() {
        let mut router = ProtocolRouter::new();
        router.register("A2A", MockHandler::new("A2A"));

        let request = create_test_request("A2A");
        let result = router.route(request);

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.protocol, "A2A");
        assert_eq!(String::from_utf8(response.body).unwrap(), "Handled by A2A");
    }

    #[test]
    fn test_protocol_router_route_protocol_not_found() {
        let router = ProtocolRouter::new();
        let request = create_test_request("UNKNOWN");

        let result = router.route(request);
        assert!(result.is_err());

        match result.unwrap_err() {
            ProtocolError::UnsupportedProtocol(protocol) => {
                assert_eq!(protocol, "UNKNOWN");
            }
            _ => panic!("Expected UnsupportedProtocol error"),
        }
    }

    #[test]
    fn test_protocol_router_route_handler_error() {
        let mut router = ProtocolRouter::new();
        router.register("ERROR", MockHandler::new_with_error("ERROR"));

        let request = create_test_request("ERROR");
        let result = router.route(request);

        assert!(result.is_err());
        match result.unwrap_err() {
            ProtocolError::Internal(msg) => {
                assert_eq!(msg, "Mock error");
            }
            _ => panic!("Expected Internal error"),
        }
    }

    #[test]
    fn test_route_debug_format() {
        let route = Route {
            pattern: "/test/*".to_string(),
            protocol: "TEST".to_string(),
            methods: vec!["GET".to_string(), "POST".to_string()],
        };

        let debug_str = format!("{:?}", route);
        assert!(debug_str.contains("/test/*"));
        assert!(debug_str.contains("TEST"));
        assert!(debug_str.contains("GET"));
        assert!(debug_str.contains("POST"));
    }

    #[test]
    fn test_route_clone() {
        let original = Route {
            pattern: "/clone/*".to_string(),
            protocol: "CLONE".to_string(),
            methods: vec!["PUT".to_string()],
        };

        let cloned = original.clone();
        assert_eq!(original.pattern, cloned.pattern);
        assert_eq!(original.protocol, cloned.protocol);
        assert_eq!(original.methods, cloned.methods);
    }

    #[test]
    fn test_route_matcher_new() {
        let matcher = RouteMatcher::new();
        assert!(matcher.routes.is_empty());
    }

    #[test]
    fn test_route_matcher_add_route() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/test".to_string(),
            "TEST".to_string(),
            vec!["GET".to_string()],
        );

        assert_eq!(matcher.routes.len(), 1);
        assert_eq!(matcher.routes[0].pattern, "/test");
        assert_eq!(matcher.routes[0].protocol, "TEST");
        assert_eq!(matcher.routes[0].methods, vec!["GET"]);
    }

    #[test]
    fn test_route_matcher_add_multiple_routes() {
        let mut matcher = RouteMatcher::new();

        matcher.add_route(
            "/a2a/*".to_string(),
            "A2A".to_string(),
            vec!["POST".to_string()],
        );
        matcher.add_route(
            "/mcp/*".to_string(),
            "MCP".to_string(),
            vec!["POST".to_string()],
        );
        matcher.add_route(
            "/rest/*".to_string(),
            "REST".to_string(),
            vec!["GET".to_string(), "POST".to_string()],
        );

        assert_eq!(matcher.routes.len(), 3);
    }

    #[test]
    fn test_route_matcher_match_exact_path() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/jsonrpc".to_string(),
            "A2A".to_string(),
            vec!["POST".to_string()],
        );

        let result = matcher.match_path("/jsonrpc", "POST");
        assert_eq!(result, Some("A2A".to_string()));

        let result = matcher.match_path("/jsonrpc", "GET");
        assert_eq!(result, None);

        let result = matcher.match_path("/different", "POST");
        assert_eq!(result, None);
    }

    #[test]
    fn test_route_matcher_match_wildcard_path() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/api/*".to_string(),
            "REST".to_string(),
            vec!["GET".to_string(), "POST".to_string()],
        );

        assert_eq!(
            matcher.match_path("/api/users", "GET"),
            Some("REST".to_string())
        );
        assert_eq!(
            matcher.match_path("/api/v1/users", "POST"),
            Some("REST".to_string())
        );
        assert_eq!(matcher.match_path("/api/", "GET"), Some("REST".to_string()));
        assert_eq!(matcher.match_path("/api", "GET"), Some("REST".to_string()));

        // Should not match
        assert_eq!(matcher.match_path("/different/users", "GET"), None);
        assert_eq!(matcher.match_path("/api/users", "DELETE"), None);
    }

    #[test]
    fn test_route_matcher_case_insensitive_methods() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/test".to_string(),
            "TEST".to_string(),
            vec!["POST".to_string()],
        );

        assert_eq!(
            matcher.match_path("/test", "POST"),
            Some("TEST".to_string())
        );
        assert_eq!(
            matcher.match_path("/test", "post"),
            Some("TEST".to_string())
        );
        assert_eq!(
            matcher.match_path("/test", "Post"),
            Some("TEST".to_string())
        );
        assert_eq!(
            matcher.match_path("/test", "pOsT"),
            Some("TEST".to_string())
        );
    }

    #[test]
    fn test_route_matcher_multiple_methods() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/multi".to_string(),
            "MULTI".to_string(),
            vec!["GET".to_string(), "POST".to_string(), "PUT".to_string()],
        );

        assert_eq!(
            matcher.match_path("/multi", "GET"),
            Some("MULTI".to_string())
        );
        assert_eq!(
            matcher.match_path("/multi", "POST"),
            Some("MULTI".to_string())
        );
        assert_eq!(
            matcher.match_path("/multi", "PUT"),
            Some("MULTI".to_string())
        );
        assert_eq!(matcher.match_path("/multi", "DELETE"), None);
    }

    #[test]
    fn test_route_matcher_first_match_wins() {
        let mut matcher = RouteMatcher::new();

        // Add conflicting routes - first one should win
        matcher.add_route(
            "/test".to_string(),
            "FIRST".to_string(),
            vec!["GET".to_string()],
        );
        matcher.add_route(
            "/test".to_string(),
            "SECOND".to_string(),
            vec!["GET".to_string()],
        );

        assert_eq!(
            matcher.match_path("/test", "GET"),
            Some("FIRST".to_string())
        );
    }

    #[test]
    fn test_route_matcher_pattern_matches_exact() {
        let matcher = RouteMatcher::new();

        assert!(matcher.pattern_matches("/exact", "/exact"));
        assert!(!matcher.pattern_matches("/exact", "/different"));
        assert!(!matcher.pattern_matches("/exact", "/exact/more"));
    }

    #[test]
    fn test_route_matcher_pattern_matches_wildcard() {
        let matcher = RouteMatcher::new();

        assert!(matcher.pattern_matches("/api/*", "/api/users"));
        assert!(matcher.pattern_matches("/api/*", "/api/v1/users"));
        assert!(matcher.pattern_matches("/api/*", "/api/"));
        assert!(matcher.pattern_matches("/api/*", "/api"));
        assert!(!matcher.pattern_matches("/api/*", "/different/users"));
        assert!(!matcher.pattern_matches("/api/*", "/ap"));
    }

    #[test]
    fn test_route_matcher_default_routes() {
        let matcher = RouteMatcher::default();

        assert_eq!(
            matcher.match_path("/jsonrpc", "POST"),
            Some("A2A".to_string())
        );
        assert_eq!(
            matcher.match_path("/a2a/test", "POST"),
            Some("A2A".to_string())
        );
        assert_eq!(
            matcher.match_path("/mcp/rpc", "POST"),
            Some("MCP".to_string())
        );
        assert_eq!(
            matcher.match_path("/rest/users", "GET"),
            Some("REST".to_string())
        );
        assert_eq!(
            matcher.match_path("/rest/users", "POST"),
            Some("REST".to_string())
        );
        assert_eq!(
            matcher.match_path("/rest/users", "PUT"),
            Some("REST".to_string())
        );
        assert_eq!(
            matcher.match_path("/rest/users", "DELETE"),
            Some("REST".to_string())
        );
        assert_eq!(
            matcher.match_path("/pubsub/topic", "POST"),
            Some("PUBSUB".to_string())
        );

        // Should not match
        assert_eq!(matcher.match_path("/unknown", "POST"), None);
        assert_eq!(matcher.match_path("/jsonrpc", "GET"), None);
        assert_eq!(matcher.match_path("/rest/users", "PATCH"), None);
    }

    #[test]
    fn test_route_matcher_empty_path() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route("".to_string(), "EMPTY".to_string(), vec!["GET".to_string()]);

        assert_eq!(matcher.match_path("", "GET"), Some("EMPTY".to_string()));
        assert_eq!(matcher.match_path("/", "GET"), None);
    }

    #[test]
    fn test_route_matcher_root_path() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route("/".to_string(), "ROOT".to_string(), vec!["GET".to_string()]);

        assert_eq!(matcher.match_path("/", "GET"), Some("ROOT".to_string()));
        assert_eq!(matcher.match_path("", "GET"), None);
    }

    #[test]
    fn test_route_matcher_no_routes() {
        let matcher = RouteMatcher::new();
        assert_eq!(matcher.match_path("/any", "GET"), None);
        assert_eq!(matcher.match_path("/any", "POST"), None);
    }

    #[test]
    fn test_route_matcher_with_complex_wildcards() {
        let mut matcher = RouteMatcher::new();
        matcher.add_route(
            "/v1/api/*".to_string(),
            "V1".to_string(),
            vec!["GET".to_string()],
        );
        matcher.add_route(
            "/v2/api/*".to_string(),
            "V2".to_string(),
            vec!["GET".to_string()],
        );

        assert_eq!(
            matcher.match_path("/v1/api/users", "GET"),
            Some("V1".to_string())
        );
        assert_eq!(
            matcher.match_path("/v2/api/users", "GET"),
            Some("V2".to_string())
        );
        assert_eq!(matcher.match_path("/v3/api/users", "GET"), None);
    }
}
