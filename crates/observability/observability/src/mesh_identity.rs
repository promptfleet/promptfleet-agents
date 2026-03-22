//! PromptFleet mesh identity helpers (stable, low-cardinality).
//!
//! This module centralizes how we derive canonical workload/external IDs used for:
//! - span attributes (`pf.source.workload`, `pf.target.workload`)
//! - derived edge metrics (span-to-metrics in the collector)
//!
//! v1 design constraints:
//! - in-mesh A2A targets are addressed by Kubernetes service DNS only
//!   (`<agent>.<ns>.svc.cluster.local` plus common shortened forms).
//! - avoid per-pod / per-request identifiers to keep cardinality safe by default.

/// Canonical workload ID prefix.
pub const WORKLOAD_PREFIX: &str = "workload:";

/// Canonical external ID prefix.
pub const EXTERNAL_PREFIX: &str = "external:";

/// Read the cluster name used for canonical IDs.
///
/// - Primary: `PF_CLUSTER_NAME`
/// - Fallback: `"local"`
pub fn cluster_name() -> String {
    std::env::var("PF_CLUSTER_NAME")
        .ok()
        .and_then(non_empty_trimmed)
        .unwrap_or_else(|| "local".to_string())
}

/// Read the current namespace used for canonical IDs.
///
/// - In-cluster/SpinKube: `SPINKUBE_NAMESPACE`
/// - Native: `PF_NAMESPACE`
/// - Fallback: `"default"`
pub fn current_namespace() -> String {
    std::env::var("SPINKUBE_NAMESPACE")
        .ok()
        .and_then(non_empty_trimmed)
        .or_else(|| {
            std::env::var("PF_NAMESPACE")
                .ok()
                .and_then(non_empty_trimmed)
        })
        .unwrap_or_else(|| "default".to_string())
}

/// Read the current service name used for canonical IDs.
///
/// - Primary: `OTEL_SERVICE_NAME` (standard)
/// - Fallback: `"unknown"`
pub fn current_service_name() -> String {
    std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .and_then(non_empty_trimmed)
        .unwrap_or_else(|| "unknown".to_string())
}

/// Create a canonical workload ID.
pub fn workload_id(cluster: &str, namespace: &str, name: &str) -> String {
    format!(
        "{WORKLOAD_PREFIX}{}/{}/{}",
        sanitize_segment(cluster),
        sanitize_segment(namespace),
        sanitize_segment(name)
    )
}

/// Create a canonical external ID (`external:<host>`).
pub fn external_id(host: &str) -> String {
    format!("{EXTERNAL_PREFIX}{}", normalize_host(host))
}

/// Derive a canonical target identifier from a peer string (typically `host[:port]`).
///
/// Rules:
/// - If host matches k8s service DNS forms, return a `workload:<cluster>/<ns>/<name>` ID.
/// - Otherwise return `external:<normalized_host>`.
pub fn target_id_from_peer(peer: &str) -> String {
    let cluster = cluster_name();
    let ns_fallback = current_namespace();

    let host = extract_host(peer);
    if host.is_empty() {
        return external_id("unknown");
    }

    if let Some((name, ns)) = parse_k8s_service_host(&host, &ns_fallback) {
        return workload_id(&cluster, &ns, &name);
    }

    external_id(&host)
}

/// Extract a host (no scheme, no path, no port) from a peer-like string.
///
/// Accepts forms like:
/// - `http://agent.default.svc.cluster.local:3000/jsonrpc`
/// - `agent.default.svc.cluster.local:3000`
/// - `agent.default.svc.cluster.local`
/// - `[2001:db8::1]:4318`
pub fn extract_host(input: &str) -> String {
    // Drop scheme if present.
    let without_scheme = input.split("://").nth(1).unwrap_or(input);
    // Drop path if present.
    let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);

    // Handle bracketed IPv6: [::1]:1234
    if let Some(rest) = host_port.strip_prefix('[') {
        if let Some((ipv6, _after)) = rest.split_once(']') {
            return ipv6.to_string();
        }
    }

    // For everything else, split on ':' and keep the first segment as host.
    // This intentionally treats raw IPv6 (without brackets) as external/unknown.
    host_port.split(':').next().unwrap_or("").to_string()
}

/// Parse Kubernetes service DNS hostnames into `(service_name, namespace)`.
///
/// Accepted v1 patterns:
/// - `<name>.<namespace>.svc.cluster.local`
/// - `<name>.<namespace>.svc`
/// - `<name>.<namespace>`
/// - `<name>` (namespace defaults to `namespace_fallback`)
pub fn parse_k8s_service_host(host: &str, namespace_fallback: &str) -> Option<(String, String)> {
    let host = host.trim();
    if host.is_empty() {
        return None;
    }

    // If it's an IPv4-ish address, treat as non-k8s (avoid junk workload IDs).
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }

    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        // Bare service name
        [name] => Some((sanitize_segment(name), sanitize_segment(namespace_fallback))),
        // name.namespace
        [name, ns] => Some((sanitize_segment(name), sanitize_segment(ns))),
        // name.namespace.svc[.*]
        [name, ns, "svc", ..] => Some((sanitize_segment(name), sanitize_segment(ns))),
        // Anything else: treat as external (e.g. api.openai.com)
        _ => None,
    }
}

/// Normalize a host for use in `external:<host>`.
///
/// - lowercases
/// - trims whitespace
/// - strips trailing dot
pub fn normalize_host(host: &str) -> String {
    let mut h = host.trim().to_ascii_lowercase();
    while h.ends_with('.') {
        h.pop();
    }
    if h.is_empty() {
        "unknown".to_string()
    } else {
        h
    }
}

fn non_empty_trimmed(v: String) -> Option<String> {
    let t = v.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

fn sanitize_segment(s: &str) -> String {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return "unknown".to_string();
    }

    // Keep DNS-label-safe characters; replace others with '-'.
    // Also collapse repeated '-' and trim ends.
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = false;
    for ch in s.chars() {
        let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
        let ch = if ok { ch } else { '-' };
        if ch == '-' {
            if prev_dash {
                continue;
            }
            prev_dash = true;
        } else {
            prev_dash = false;
        }
        out.push(ch);
    }
    out.trim_matches('-').to_string().if_empty("unknown")
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> Self;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> Self {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_k8s_service_dns_full() {
        let out = parse_k8s_service_host("llm-coordinator.default.svc.cluster.local", "x");
        assert_eq!(
            out,
            Some(("llm-coordinator".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn parses_k8s_service_dns_short() {
        let out = parse_k8s_service_host("llm-coordinator.default.svc", "x");
        assert_eq!(
            out,
            Some(("llm-coordinator".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn parses_name_namespace() {
        let out = parse_k8s_service_host("llm-coordinator.default", "x");
        assert_eq!(
            out,
            Some(("llm-coordinator".to_string(), "default".to_string()))
        );
    }

    #[test]
    fn parses_bare_name_uses_fallback_ns() {
        let out = parse_k8s_service_host("llm-coordinator", "pf-agents");
        assert_eq!(
            out,
            Some(("llm-coordinator".to_string(), "pf-agents".to_string()))
        );
    }

    #[test]
    fn rejects_external_dns() {
        let out = parse_k8s_service_host("api.openai.com", "x");
        assert_eq!(out, None);
    }

    #[test]
    fn extract_host_strips_scheme_path_port() {
        assert_eq!(
            extract_host("http://llm-coordinator.default.svc.cluster.local:3000/jsonrpc"),
            "llm-coordinator.default.svc.cluster.local".to_string()
        );
    }
}
