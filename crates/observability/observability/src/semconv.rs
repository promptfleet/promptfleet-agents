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
