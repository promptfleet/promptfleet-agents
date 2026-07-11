use std::collections::{HashMap, HashSet};

/// Dedicated short-lived credential for a downstream MCP call. This must not
/// be treated as the runtime transport's own Authorization header.
pub const MCP_CALLER_AUTHORIZATION_HEADER: &str = "x-pf-mcp-caller-authorization";

const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

const TRANSPORT_MANAGED_HEADERS: &[&str] = &["host", "content-length", "accept-encoding"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardedHeaders {
    headers: HashMap<String, String>,
}

impl ForwardedHeaders {
    pub fn new(headers: HashMap<String, String>) -> Self {
        Self { headers }
    }

    pub fn as_map(&self) -> &HashMap<String, String> {
        &self.headers
    }

    pub fn into_map(self) -> HashMap<String, String> {
        self.headers
    }
}

pub fn sanitize_header_map(headers: &http::HeaderMap) -> ForwardedHeaders {
    let mut raw = HashMap::new();
    for (name, value) in headers {
        if let Ok(value) = value.to_str() {
            raw.insert(name.as_str().to_string(), value.to_string());
        }
    }
    sanitize_headers(&raw)
}

pub fn sanitize_headers(headers: &HashMap<String, String>) -> ForwardedHeaders {
    let mut excluded = connection_listed_headers(headers);
    excluded.extend(HOP_BY_HOP_HEADERS.iter().map(|h| h.to_string()));
    excluded.extend(TRANSPORT_MANAGED_HEADERS.iter().map(|h| h.to_string()));

    let mut out = HashMap::new();
    for (name, value) in headers {
        let lowered = name.to_ascii_lowercase();
        if lowered == "content-type" {
            continue;
        }
        if excluded.contains(&lowered) {
            continue;
        }
        out.insert(name.clone(), value.clone());
    }

    ForwardedHeaders::new(out)
}

fn connection_listed_headers(headers: &HashMap<String, String>) -> HashSet<String> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_headers_preserves_unknown_and_auth_headers() {
        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer token".to_string()),
            ("traceparent".to_string(), "00-abc-def-01".to_string()),
            ("x-pf-tid".to_string(), "T-123".to_string()),
            ("x-custom".to_string(), "value".to_string()),
        ]);

        let out = sanitize_headers(&headers).into_map();
        assert_eq!(
            out.get("Authorization").map(String::as_str),
            Some("Bearer token")
        );
        assert_eq!(
            out.get("traceparent").map(String::as_str),
            Some("00-abc-def-01")
        );
        assert_eq!(out.get("x-pf-tid").map(String::as_str), Some("T-123"));
        assert_eq!(out.get("x-custom").map(String::as_str), Some("value"));
    }

    #[test]
    fn sanitize_headers_strips_hop_by_hop_and_transport_managed_headers() {
        let headers = HashMap::from([
            ("Connection".to_string(), "x-remove-me".to_string()),
            ("keep-alive".to_string(), "timeout=5".to_string()),
            ("host".to_string(), "example.com".to_string()),
            ("content-length".to_string(), "99".to_string()),
            ("content-type".to_string(), "application/json".to_string()),
            ("x-remove-me".to_string(), "bye".to_string()),
            ("x-keep-me".to_string(), "hello".to_string()),
        ]);

        let out = sanitize_headers(&headers).into_map();
        assert!(!out.contains_key("Connection"));
        assert!(!out.contains_key("keep-alive"));
        assert!(!out.contains_key("host"));
        assert!(!out.contains_key("content-length"));
        assert!(!out.contains_key("content-type"));
        assert!(!out.contains_key("x-remove-me"));
        assert_eq!(out.get("x-keep-me").map(String::as_str), Some("hello"));
    }
}
