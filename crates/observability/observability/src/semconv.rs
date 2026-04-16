//! Minimal semantic conventions used across PromptFleet SDK components.
//!
//! Goals:
//! - **Low cardinality** by default (bounded sets for labels/attrs)
//! - **Stable names** (safe for dashboards/alerts)
//! - **Backend-agnostic** (OTEL / Prometheus)

/// Common span names.
pub mod span {
    /// Incoming A2A request handling.
    pub const A2A_SERVER: &str = "a2a.server";
    /// Outgoing A2A request.
    pub const A2A_CLIENT: &str = "a2a.client";
    /// LLM request (chat/completions/tools loop).
    pub const LLM_REQUEST: &str = "llm.request";
    /// Tool execution request inside the agent runtime.
    pub const TOOL_CALL: &str = "tool.call";
}

/// Common attribute keys (keep stable; values should be low-cardinality).
pub mod attr {
    /// Component name.
    ///
    /// Recommended bounded values:
    /// - `a2a_server`
    /// - `a2a_client`
    /// - `llm_client`
    /// - `sdk`
    pub const COMPONENT: &str = "component";
    /// Operation name (bounded set), e.g. A2A JSON-RPC method name.
    pub const OPERATION: &str = "operation";
    /// Remote peer service/agent name (bounded set; do not use random IDs).
    pub const PEER_SERVICE: &str = "peer.service";

    /// High-level outcome for spans and metrics (low-cardinality).
    ///
    /// Values: `"ok"` | `"error"`.
    pub const STATUS: &str = "status";
    /// Error type/category (keep bounded-ish), e.g. `"timeout"`, `"transport"`, `"rate_limit"`.
    pub const ERROR_TYPE: &str = "error.type";
    /// Error message (optional; be careful with PII / cardinality).
    pub const ERROR_MESSAGE: &str = "error.message";

    /// LLM provider (e.g. `openai`, `anthropic`, `local`).
    pub const LLM_PROVIDER: &str = "llm.provider";
    /// LLM model name (keep bounded / configured).
    pub const LLM_MODEL: &str = "llm.model";
    /// LLM operation (bounded set), e.g. `chat_completions`, `embeddings`.
    pub const LLM_OPERATION: &str = "llm.operation";
    /// Token counts (numbers as strings).
    pub const LLM_TOKENS_INPUT: &str = "llm.tokens.input";
    pub const LLM_TOKENS_OUTPUT: &str = "llm.tokens.output";
    /// Tool name (bounded by the registered tool surface).
    pub const TOOL_NAME: &str = "tool.name";
    /// Tool execution surface / family (bounded).
    ///
    /// Recommended values:
    /// - `function`
    /// - `mcp`
    /// - `http`
    /// - `a2a`
    /// - `a2a_delegate`
    /// - `interaction`
    /// - `skill`
    pub const TOOL_KIND: &str = "tool.kind";

    // ---------------------------------------------------------------------
    // OTEL GenAI semantic conventions
    // ---------------------------------------------------------------------

    /// The GenAI system/provider handling the request.
    pub const GEN_AI_SYSTEM: &str = "gen_ai.system";
    /// The requested GenAI model.
    pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
    /// The GenAI operation name.
    pub const GEN_AI_OPERATION_NAME: &str = "gen_ai.operation.name";
    /// Tokens used in the prompt/input.
    pub const GEN_AI_USAGE_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
    /// Tokens used in the completion/output.
    pub const GEN_AI_USAGE_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
    /// Final finish reason(s) reported by the provider.
    pub const GEN_AI_RESPONSE_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";
    /// Final model reported by the provider, if available.
    pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";

    // ---------------------------------------------------------------------
    // PromptFleet Mesh / A2A graph attributes (stable, low-cardinality)
    // ---------------------------------------------------------------------

    /// Canonical source workload identifier for mesh graph derivation.
    ///
    /// Value format: `workload:<cluster>/<namespace>/<name>`
    pub const PF_SOURCE_WORKLOAD: &str = "pf.source.workload";

    /// Canonical target identifier for mesh graph derivation.
    ///
    /// Value format:
    /// - `workload:<cluster>/<namespace>/<name>` for in-mesh A2A calls
    /// - `external:<normalized_host>` for external dependencies
    pub const PF_TARGET_WORKLOAD: &str = "pf.target.workload";

    /// Bounded outcome enum for spans and derived edge metrics.
    ///
    /// Values: `"ok" | "error" | "timeout" | "cancelled" | "invalid"`.
    pub const PF_OUTCOME: &str = "pf.outcome";

    /// Bounded edge kind enum for mesh edges.
    ///
    /// Values: `"a2a" | "external"`.
    pub const PF_KIND: &str = "pf.kind";

    // ---------------------------------------------------------------------
    // OTEL semantic conventions we intentionally align with
    // ---------------------------------------------------------------------

    /// RPC system identifier (e.g. `"jsonrpc"`).
    pub const RPC_SYSTEM: &str = "rpc.system";

    /// RPC method name (bounded by API surface).
    pub const RPC_METHOD: &str = "rpc.method";
}

/// Common metric names (low-cardinality labels only).
pub mod metric {
    /// Counter: total A2A requests.
    ///
    /// Labels: `{component, operation, status}`
    pub const A2A_REQUESTS_TOTAL: &str = "a2a_requests_total";
    /// Histogram: A2A request latency (milliseconds).
    ///
    /// Labels: `{component, operation, status}`
    pub const A2A_LATENCY_MS: &str = "a2a_latency_ms";

    /// Counter: total LLM requests.
    ///
    /// Labels: `{provider, model, operation, status}`
    pub const LLM_REQUESTS_TOTAL: &str = "llm_requests_total";
    /// Histogram: LLM request latency (milliseconds).
    ///
    /// Labels: `{provider, model, operation, status}`
    pub const LLM_LATENCY_MS: &str = "llm_latency_ms";
    /// Counter: total LLM tokens (split by direction).
    ///
    /// Labels: `{provider, model, direction}` where `direction ∈ {"input","output"}`.
    pub const LLM_TOKENS_TOTAL: &str = "llm_tokens_total";
}

/// Fixed allowlist of metric label keys to keep cardinality bounded.
///
/// The facade will **drop** labels not in this list (best-effort) to avoid accidental explosions.
pub const METRIC_LABEL_ALLOWLIST: &[&str] = &[
    // Service identity labels (used by some backends / internal metrics).
    "app",
    "version",
    "namespace",
    // A2A / SDK.
    attr::COMPONENT,
    attr::OPERATION,
    attr::STATUS,
    // LLM.
    "provider",
    "model",
    "direction",
];

/// Filter labels to the allowlist (stable order preserved).
pub fn filter_metric_labels<'a>(labels: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    labels
        .iter()
        .copied()
        .filter(|(k, _)| METRIC_LABEL_ALLOWLIST.contains(k))
        .collect()
}

/// Convenience constants for common low-cardinality values.
pub mod value {
    pub const STATUS_OK: &str = "ok";
    pub const STATUS_ERROR: &str = "error";

    pub const DIRECTION_INPUT: &str = "input";
    pub const DIRECTION_OUTPUT: &str = "output";

    // Mesh / graph outcomes (bounded).
    pub const OUTCOME_OK: &str = "ok";
    pub const OUTCOME_ERROR: &str = "error";
    pub const OUTCOME_TIMEOUT: &str = "timeout";
    pub const OUTCOME_CANCELLED: &str = "cancelled";
    pub const OUTCOME_INVALID: &str = "invalid";

    // Mesh / graph kinds (bounded).
    pub const KIND_A2A: &str = "a2a";
    pub const KIND_EXTERNAL: &str = "external";

    // RPC systems (bounded).
    pub const RPC_SYSTEM_JSONRPC: &str = "jsonrpc";
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Allowlist filtering ---

    #[test]
    fn test_filter_keeps_only_allowed_labels() {
        let labels = vec![
            ("component", "sdk"),
            ("secret_key", "hunter2"),
            ("status", "ok"),
            ("random", "noise"),
            ("model", "gpt-4"),
        ];

        let filtered = filter_metric_labels(&labels);

        let keys: Vec<&str> = filtered.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"component"));
        assert!(keys.contains(&"status"));
        assert!(keys.contains(&"model"));
        assert!(!keys.contains(&"secret_key"));
        assert!(!keys.contains(&"random"));
    }

    #[test]
    fn test_filter_preserves_input_order() {
        let labels = vec![
            ("status", "ok"),
            ("component", "sdk"),
            ("model", "gpt-4"),
            ("operation", "chat"),
        ];

        let filtered = filter_metric_labels(&labels);
        let keys: Vec<&str> = filtered.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["status", "component", "model", "operation"]);
    }

    #[test]
    fn test_filter_drops_all_when_none_allowed() {
        let labels = vec![("secret", "val"), ("internal_id", "123")];
        let filtered = filter_metric_labels(&labels);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_empty_input() {
        let filtered = filter_metric_labels(&[]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_keeps_values_intact() {
        let labels = vec![("component", "a2a_server"), ("status", "error")];
        let filtered = filter_metric_labels(&labels);
        assert_eq!(filtered[0], ("component", "a2a_server"));
        assert_eq!(filtered[1], ("status", "error"));
    }

    // --- Allowlist contents ---

    #[test]
    fn test_allowlist_contains_expected_keys() {
        for key in &[
            "app",
            "version",
            "namespace",
            "component",
            "operation",
            "status",
            "provider",
            "model",
            "direction",
        ] {
            assert!(
                METRIC_LABEL_ALLOWLIST.contains(key),
                "Missing expected key: {}",
                key
            );
        }
    }

    #[test]
    fn test_allowlist_does_not_contain_high_cardinality() {
        for key in &["trace_id", "span_id", "request_id", "user_id", "ip"] {
            assert!(
                !METRIC_LABEL_ALLOWLIST.contains(key),
                "Allowlist should not contain high-cardinality key: {}",
                key
            );
        }
    }

    // --- Constant stability ---

    #[test]
    fn test_span_constants_are_stable() {
        assert_eq!(span::A2A_SERVER, "a2a.server");
        assert_eq!(span::A2A_CLIENT, "a2a.client");
        assert_eq!(span::LLM_REQUEST, "llm.request");
        assert_eq!(span::TOOL_CALL, "tool.call");
    }

    #[test]
    fn test_attr_constants_are_stable() {
        assert_eq!(attr::COMPONENT, "component");
        assert_eq!(attr::OPERATION, "operation");
        assert_eq!(attr::STATUS, "status");
        assert_eq!(attr::PEER_SERVICE, "peer.service");
        assert_eq!(attr::LLM_PROVIDER, "llm.provider");
        assert_eq!(attr::LLM_MODEL, "llm.model");
        assert_eq!(attr::LLM_OPERATION, "llm.operation");
        assert_eq!(attr::LLM_TOKENS_INPUT, "llm.tokens.input");
        assert_eq!(attr::LLM_TOKENS_OUTPUT, "llm.tokens.output");
        assert_eq!(attr::TOOL_NAME, "tool.name");
        assert_eq!(attr::TOOL_KIND, "tool.kind");
        assert_eq!(attr::GEN_AI_SYSTEM, "gen_ai.system");
        assert_eq!(attr::GEN_AI_REQUEST_MODEL, "gen_ai.request.model");
        assert_eq!(attr::GEN_AI_OPERATION_NAME, "gen_ai.operation.name");
        assert_eq!(attr::GEN_AI_USAGE_INPUT_TOKENS, "gen_ai.usage.input_tokens");
        assert_eq!(attr::GEN_AI_USAGE_OUTPUT_TOKENS, "gen_ai.usage.output_tokens");
        assert_eq!(
            attr::GEN_AI_RESPONSE_FINISH_REASONS,
            "gen_ai.response.finish_reasons"
        );
        assert_eq!(attr::GEN_AI_RESPONSE_MODEL, "gen_ai.response.model");
        assert_eq!(attr::PF_SOURCE_WORKLOAD, "pf.source.workload");
        assert_eq!(attr::PF_TARGET_WORKLOAD, "pf.target.workload");
        assert_eq!(attr::PF_OUTCOME, "pf.outcome");
        assert_eq!(attr::PF_KIND, "pf.kind");
        assert_eq!(attr::RPC_SYSTEM, "rpc.system");
        assert_eq!(attr::RPC_METHOD, "rpc.method");
    }

    #[test]
    fn test_metric_constants_are_stable() {
        assert_eq!(metric::A2A_REQUESTS_TOTAL, "a2a_requests_total");
        assert_eq!(metric::A2A_LATENCY_MS, "a2a_latency_ms");
        assert_eq!(metric::LLM_REQUESTS_TOTAL, "llm_requests_total");
        assert_eq!(metric::LLM_LATENCY_MS, "llm_latency_ms");
        assert_eq!(metric::LLM_TOKENS_TOTAL, "llm_tokens_total");
    }

    #[test]
    fn test_value_constants_are_stable() {
        assert_eq!(value::STATUS_OK, "ok");
        assert_eq!(value::STATUS_ERROR, "error");
        assert_eq!(value::DIRECTION_INPUT, "input");
        assert_eq!(value::DIRECTION_OUTPUT, "output");
        assert_eq!(value::OUTCOME_OK, "ok");
        assert_eq!(value::OUTCOME_TIMEOUT, "timeout");
        assert_eq!(value::OUTCOME_CANCELLED, "cancelled");
        assert_eq!(value::OUTCOME_INVALID, "invalid");
        assert_eq!(value::KIND_A2A, "a2a");
        assert_eq!(value::KIND_EXTERNAL, "external");
        assert_eq!(value::RPC_SYSTEM_JSONRPC, "jsonrpc");
    }
}
