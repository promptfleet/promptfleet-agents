use std::sync::{Arc, Mutex};

#[cfg(any(feature = "otel", feature = "prometheus"))]
use observability_core::ObservabilityPlugin;
use observability_core::traits::LogLevel;
use observability_core::{
    ObservabilityConfig as CoreLoggingConfig, ObservabilityManager, ObservabilityResult,
};
use web_time::{Duration, Instant};

#[cfg(feature = "logging")]
use serde_json::Value as JsonValue;

use std::collections::HashMap;

#[cfg(feature = "config")]
use serde_json::Value as ConfigValue;

#[cfg(feature = "serde")]
mod duration_millis {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use web_time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        d.as_millis().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}

/// Convenience result type for the facade.
pub type ObsResult<T> = ObservabilityResult<T>;

/// A small, stable interface that subcomponents can depend on.
///
/// Prefer taking `SharedObs` (`Arc<dyn ObsHandle>`) in A2A/LLM components.
pub trait ObsHandle: Send + Sync {
    /// Start a span. The returned guard ends the span on drop.
    fn span(&self, name: &str, attrs: &[(&str, &str)]) -> observability_core::SpanGuard;

    /// Record a metric (counter/histogram/gauge depending on backend configuration).
    fn metric(&self, name: &str, value: f64, labels: &[(&str, &str)]);

    /// Emit a structured log event (best effort).
    #[cfg(feature = "logging")]
    fn log(&self, level: LogLevel, message: &str, fields: &JsonValue) -> ObsResult<()>;

    /// Emit a log event with low-cardinality key/value fields.
    ///
    /// This method is intentionally **always available** (no serde types in the signature).
    /// When the `logging` feature is disabled, it becomes a best-effort no-op.
    fn log_kv(&self, level: LogLevel, message: &str, fields: &[(&str, &str)]);

    /// Flush any buffered telemetry (best effort).
    fn flush(&self) -> ObsResult<()>;

    /// Health status of configured backends (best effort; does not do network probes).
    fn health(&self) -> ObsHealth;
}

pub type SharedObs = Arc<dyn ObsHandle>;

fn default_otel_metrics_enabled() -> bool {
    true
}

/// Unified configuration for the happy path.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub struct ObservabilityConfig {
    /// Service identity (used for correlation fields and backend configs).
    pub service_name: String,
    pub service_version: String,
    pub service_namespace: String,

    /// Global structured logging configuration (installed once per process).
    pub logging: CoreLoggingConfig,

    /// Optional OTEL exporter configuration.
    pub otel: OtelConfig,

    /// Optional Prometheus exporter configuration.
    pub prometheus: PrometheusConfig,
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            service_name: "promptfleet-agent".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: "default".to_string(),
            logging: CoreLoggingConfig::default(),
            otel: OtelConfig::disabled(),
            prometheus: PrometheusConfig::disabled(),
        }
    }
}

impl ObservabilityConfig {
    pub fn with_service(
        mut self,
        name: impl Into<String>,
        version: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Self {
        self.service_name = name.into();
        self.service_version = version.into();
        self.service_namespace = namespace.into();
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub struct OtelConfig {
    pub enabled: bool,
    #[cfg_attr(feature = "serde", serde(default = "default_otel_metrics_enabled"))]
    pub metrics_enabled: bool,
    pub otlp_endpoint: String,
    pub batch_size: usize,
    #[cfg_attr(feature = "serde", serde(with = "duration_millis"))]
    pub export_timeout: Duration,
    pub sampling: OtelSampling,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OtelSampling {
    AlwaysOn,
    AlwaysOff,
    TraceIdRatio(f64),
    ParentBased,
}

#[cfg(feature = "serde")]
impl serde::Serialize for OtelSampling {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            OtelSampling::AlwaysOn => serializer.serialize_str("always_on"),
            OtelSampling::AlwaysOff => serializer.serialize_str("always_off"),
            OtelSampling::ParentBased => serializer.serialize_str("parent_based"),
            OtelSampling::TraceIdRatio(r) => serializer.serialize_str(&format!("ratio:{}", r)),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for OtelSampling {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "always_on" | "alwayson" | "on" => Ok(OtelSampling::AlwaysOn),
            "always_off" | "alwaysoff" | "off" => Ok(OtelSampling::AlwaysOff),
            "parent_based" | "parentbased" => Ok(OtelSampling::ParentBased),
            other => {
                let ratio_str = other.strip_prefix("ratio:").unwrap_or(other);
                ratio_str
                    .parse::<f64>()
                    .map(OtelSampling::TraceIdRatio)
                    .map_err(|_| {
                        serde::de::Error::custom(format!(
                            "unknown sampling strategy: '{}' (expected always_on, always_off, parent_based, ratio:<f64>, or <f64>)",
                            s
                        ))
                    })
            }
        }
    }
}

impl OtelConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            metrics_enabled: default_otel_metrics_enabled(),
            otlp_endpoint: "http://localhost:4317".to_string(),
            batch_size: 512,
            export_timeout: Duration::from_secs(30),
            sampling: OtelSampling::TraceIdRatio(0.1),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub struct PrometheusConfig {
    pub enabled: bool,
    pub pushgateway_endpoint: Option<String>,
    pub job_name: String,
    pub instance: String,
    #[cfg_attr(feature = "serde", serde(with = "duration_millis"))]
    pub push_interval: Duration,
    pub cardinality_reduction: bool,
    pub max_cardinality: usize,
}

impl PrometheusConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            pushgateway_endpoint: Some("http://localhost:9091".to_string()),
            job_name: "promptfleet-agent".to_string(),
            instance: "localhost:9090".to_string(),
            push_interval: Duration::from_secs(30),
            cardinality_reduction: true,
            max_cardinality: 10_000,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObsHealth {
    pub logging: bool,
    pub otel: Option<bool>,
    pub prometheus: Option<bool>,
    pub notes: Vec<String>,
}

struct ObsInner {
    _manager: ObservabilityManager,

    #[cfg_attr(not(any(feature = "otel", feature = "prometheus")), allow(dead_code))]
    otel_configured: bool,
    #[cfg_attr(not(any(feature = "otel", feature = "prometheus")), allow(dead_code))]
    otel_metrics_enabled: bool,
    #[cfg_attr(not(feature = "prometheus"), allow(dead_code))]
    prometheus_configured: bool,
    init_notes: Vec<String>,

    flush_interval: Duration,
    last_flush: Mutex<Option<Instant>>,

    #[cfg(feature = "otel")]
    otel: Option<otel::OtelManager>,

    #[cfg(feature = "prometheus")]
    prometheus: Option<obs_prometheus::PrometheusManager>,
}

/// Main facade handle. Cloneable and cheap to pass around.
#[derive(Clone)]
pub struct Obs {
    #[allow(dead_code)]
    inner: Arc<ObsInner>,
}

impl Obs {
    /// Create a no-op handle (spans/metrics/logging are best-effort no-ops).
    ///
    /// Useful for tests and for components that want an `Obs` even when
    /// observability is not configured.
    pub fn noop() -> Self {
        // Keep the inner struct minimal; don't call the singleton logger initializer.
        // The handle will still compile and be safe to pass around.
        Self {
            inner: Arc::new(ObsInner {
                _manager: ObservabilityManager::default(),
                otel_configured: false,
                otel_metrics_enabled: false,
                prometheus_configured: false,
                init_notes: vec![],
                flush_interval: Duration::from_secs(
                    observability_core::DEFAULT_FLUSH_INTERVAL_SECS,
                ),
                last_flush: Mutex::new(None),
                #[cfg(feature = "otel")]
                otel: None,
                #[cfg(feature = "prometheus")]
                prometheus: None,
            }),
        }
    }

    /// Initialize from process environment.
    ///
    /// This intentionally stays minimal: it uses OTEL/Prometheus env helpers from
    /// backend crates when those features are enabled.
    pub fn init_from_env() -> ObsResult<Self> {
        Self::init(ObservabilityConfig::from_env())
    }

    /// Extract W3C trace context from inbound headers.
    pub fn extract_context(
        headers: &HashMap<String, String>,
    ) -> ObsResult<Option<observability_core::W3CTraceContext>> {
        observability_core::HeaderExtractor(headers).extract()
    }

    /// Inject W3C trace context into outbound headers.
    pub fn inject_context(
        headers: &mut HashMap<String, String>,
        context: &observability_core::W3CTraceContext,
    ) {
        observability_core::HeaderInjector(headers).inject(context)
    }

    /// Initialize global logging once and configure optional backends.
    ///
    /// Call this **once** during agent startup and pass the returned handle into
    /// all subcomponents (A2A client/server, LLM client, etc.).
    pub fn init(mut cfg: ObservabilityConfig) -> ObsResult<Self> {
        // Ensure service identity is present in default structured fields.
        #[cfg(feature = "logging")]
        {
            cfg.logging.default_context.insert(
                "service.name".to_string(),
                serde_json::Value::String(cfg.service_name.clone()),
            );
            cfg.logging.default_context.insert(
                "service.version".to_string(),
                serde_json::Value::String(cfg.service_version.clone()),
            );
            cfg.logging.default_context.insert(
                "service.namespace".to_string(),
                serde_json::Value::String(cfg.service_namespace.clone()),
            );
        }

        let mut manager = ObservabilityManager::new(cfg.logging)?;
        manager.initialize()?;

        #[allow(unused_mut)]
        let mut init_notes = Vec::<String>::new();

        let flush_interval = flush_interval_from_env();

        #[cfg(feature = "otel")]
        let otel = if cfg.otel.enabled {
            let base = otel::OtelConfig::builder()
                .with_otlp_endpoint(cfg.otel.otlp_endpoint)
                .with_service_name(cfg.service_name.clone())
                .with_service_version(cfg.service_version.clone())
                .with_service_namespace(cfg.service_namespace.clone())
                .with_batch_size(cfg.otel.batch_size)
                .with_export_timeout_secs(cfg.otel.export_timeout.as_secs())
                .with_sampling_strategy(match cfg.otel.sampling {
                    OtelSampling::AlwaysOn => otel::SamplingStrategy::AlwaysOn,
                    OtelSampling::AlwaysOff => otel::SamplingStrategy::AlwaysOff,
                    OtelSampling::TraceIdRatio(r) => otel::SamplingStrategy::TraceIdRatio(r),
                    OtelSampling::ParentBased => otel::SamplingStrategy::parent_based(),
                })
                .build();
            match otel::OtelManager::from_config(otel::OtelExtensionConfig::with_base(base)) {
                Ok(mgr) => Some(mgr),
                Err(e) => {
                    init_notes.push(format!("otel init failed (best-effort): {}", e));
                    None
                }
            }
        } else {
            None
        };

        #[cfg(feature = "prometheus")]
        let prometheus = if cfg.prometheus.enabled {
            let base = obs_prometheus::PrometheusConfig {
                pushgateway_endpoint: cfg.prometheus.pushgateway_endpoint,
                job_name: cfg.prometheus.job_name,
                instance: cfg.prometheus.instance,
                push_interval: cfg.prometheus.push_interval,
                cardinality_reduction: cfg.prometheus.cardinality_reduction,
                max_cardinality: cfg.prometheus.max_cardinality,
                hierarchical_federation: true,
                global_labels: std::collections::HashMap::new(),
            };
            let ext = obs_prometheus::PrometheusExtensionConfig::with_base(base);
            match obs_prometheus::PrometheusManager::from_config(ext) {
                Ok(mgr) => Some(mgr),
                Err(e) => {
                    init_notes.push(format!("prometheus init failed (best-effort): {}", e));
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            inner: Arc::new(ObsInner {
                _manager: manager,
                otel_configured: cfg.otel.enabled,
                otel_metrics_enabled: cfg.otel.enabled && cfg.otel.metrics_enabled,
                prometheus_configured: cfg.prometheus.enabled,
                init_notes,
                flush_interval,
                last_flush: Mutex::new(None),
                #[cfg(feature = "otel")]
                otel,
                #[cfg(feature = "prometheus")]
                prometheus,
            }),
        })
    }

    /// Flush buffered telemetry at most once per configured interval.
    ///
    /// This is intended for WASM request-driven workloads (SpinKube): call at the end of
    /// request handling to ensure spans/metrics/logs are exported within bounded time
    /// without flushing on every single request.
    pub fn maybe_flush(&self) -> ObsResult<()> {
        let now = Instant::now();
        let should_flush = {
            let mut last = self.inner.last_flush.lock().unwrap();
            match *last {
                None => {
                    *last = Some(now);
                    true
                }
                Some(prev) => {
                    if now.duration_since(prev) >= self.inner.flush_interval {
                        *last = Some(now);
                        true
                    } else {
                        false
                    }
                }
            }
        };

        if should_flush {
            self.flush()?;
        }
        Ok(())
    }

    pub fn shared(self) -> SharedObs {
        Arc::new(self)
    }

    /// Returns the underlying OTEL plugin (if enabled and initialized).
    #[cfg(feature = "otel")]
    pub fn otel_plugin(&self) -> Option<&otel::Otel> {
        self.inner.otel.as_ref().and_then(|m| m.plugin())
    }

    /// Returns the underlying Prometheus plugin (if enabled and initialized).
    #[cfg(feature = "prometheus")]
    pub fn prometheus_plugin(&self) -> Option<&obs_prometheus::Prometheus> {
        self.inner.prometheus.as_ref().and_then(|m| m.plugin())
    }
}

fn flush_interval_from_env() -> Duration {
    const KEY: &str = "PF_OBS_FLUSH_INTERVAL_MS";

    if let Ok(v) = std::env::var(KEY) {
        if let Ok(ms) = v.trim().parse::<u64>() {
            return Duration::from_millis(ms);
        }
    }

    Duration::from_secs(observability_core::DEFAULT_FLUSH_INTERVAL_SECS)
}

impl ObsHandle for Obs {
    #[allow(unused_variables)]
    fn span(&self, name: &str, attrs: &[(&str, &str)]) -> observability_core::SpanGuard {
        // OTEL-only tracing: spans are emitted only when OTEL is available.
        #[cfg(feature = "otel")]
        if let Some(otel) = self.otel_plugin() {
            return otel.start_span(name, attrs);
        }

        observability_core::SpanGuard::no_op()
    }

    #[allow(unused_variables)]
    fn metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        // Enforce a fixed allowlist to prevent accidental high-cardinality explosions.
        // Best-effort: unknown labels are dropped.
        let labels = crate::filter_metric_labels(labels);

        // Fan-out: if multiple backends are enabled, emit to all.
        #[cfg(feature = "otel")]
        if self.inner.otel_metrics_enabled {
            if let Some(otel) = self.otel_plugin() {
                otel.record_metric(name, value, &labels);
            }
        }

        #[cfg(feature = "prometheus")]
        if let Some(prom) = self.prometheus_plugin() {
            prom.record_metric(name, value, &labels);
        }
    }

    #[cfg(feature = "logging")]
    fn log(&self, level: LogLevel, message: &str, fields: &JsonValue) -> ObsResult<()> {
        // Use core's global logger singleton if available (best-effort).
        observability_core::extension::convenience::log_with_fields(level, message, fields.clone())
    }

    fn log_kv(&self, level: LogLevel, message: &str, fields: &[(&str, &str)]) {
        #[cfg(feature = "logging")]
        {
            let mut map = serde_json::Map::new();
            for (k, v) in fields {
                map.insert(
                    (*k).to_string(),
                    serde_json::Value::String((*v).to_string()),
                );
            }
            let _ = self.log(level, message, &serde_json::Value::Object(map));
        }

        #[cfg(not(feature = "logging"))]
        {
            let _ = (level, message, fields);
        }
    }

    fn flush(&self) -> ObsResult<()> {
        #[cfg(feature = "otel")]
        if let Some(otel) = self.otel_plugin() {
            let _ = otel.flush();
        }

        #[cfg(feature = "prometheus")]
        if let Some(prom) = self.prometheus_plugin() {
            let _ = prom.flush();
        }

        Ok(())
    }

    fn health(&self) -> ObsHealth {
        #[allow(unused_mut)]
        let mut out = ObsHealth {
            logging: true,
            notes: self.inner.init_notes.clone(),
            ..Default::default()
        };

        #[cfg(feature = "otel")]
        {
            if let Some(mgr) = &self.inner.otel {
                let (ok, msg) = mgr.health_status();
                out.otel = Some(ok);
                out.notes.push(msg);
            } else if self.inner.otel_configured {
                out.otel = Some(false);
                out.notes
                    .push("OTEL configured but not initialized".to_string());
            } else {
                out.otel = None;
            }
        }

        #[cfg(feature = "prometheus")]
        {
            if let Some(mgr) = &self.inner.prometheus {
                let (ok, msg) = mgr.health_status();
                out.prometheus = Some(ok);
                out.notes.push(msg);
            } else if self.inner.prometheus_configured {
                out.prometheus = Some(false);
                out.notes
                    .push("Prometheus configured but not initialized".to_string());
            } else {
                out.prometheus = None;
            }
        }

        out
    }
}

impl ObservabilityConfig {
    /// Build an `ObservabilityConfig` from environment variables.
    ///
    /// Supported (minimal) env vars:
    /// - Service identity: `OTEL_SERVICE_NAME`, `OTEL_SERVICE_VERSION`, `OTEL_SERVICE_NAMESPACE`
    /// - Logging:
    ///   - Filtering: `OBS_LOG_LEVEL` (PromptFleet) or `RUST_LOG` (Rust ecosystem)
    ///   - Format: `PF_LOG_FORMAT` (PromptFleet) or `OBS_LOG_FORMAT` (back-compat) or `LOG_FORMAT` (alias)
    ///   - Flags: `OBS_LOG_STRUCTURED`, `OBS_LOG_CONTEXT`
    /// - OTEL: `OBS_OTEL_ENABLED`, `OBS_OTEL_METRICS_ENABLED`, `OTEL_EXPORTER_OTLP_ENDPOINT`, `OBS_OTEL_SAMPLING`, `OBS_OTEL_SAMPLING_RATIO`
    /// - Prometheus: `OBS_PROMETHEUS_ENABLED`, `PROMETHEUS_PUSHGATEWAY`, `PROMETHEUS_JOB_NAME`, `PROMETHEUS_INSTANCE`
    pub fn from_env() -> Self {
        fn env_bool(key: &str) -> Option<bool> {
            std::env::var(key).ok().and_then(|v| {
                let v = v.trim().to_ascii_lowercase();
                match v.as_str() {
                    "1" | "true" | "yes" | "y" | "on" => Some(true),
                    "0" | "false" | "no" | "n" | "off" => Some(false),
                    _ => None,
                }
            })
        }

        let mut cfg = Self::default();

        if let Ok(name) = std::env::var("OTEL_SERVICE_NAME") {
            cfg.service_name = name;
        }
        if let Ok(version) = std::env::var("OTEL_SERVICE_VERSION") {
            cfg.service_version = version;
        }
        if let Ok(ns) = std::env::var("OTEL_SERVICE_NAMESPACE") {
            cfg.service_namespace = ns;
        }

        // Logging level: prefer PromptFleet knob, otherwise fall back to standard `RUST_LOG`.
        if let Ok(level) = std::env::var("OBS_LOG_LEVEL") {
            cfg.logging.level = level;
        } else if let Ok(rust_log) = std::env::var("RUST_LOG") {
            // Parse only the "global" level from RUST_LOG (best-effort).
            // Examples:
            // - "info" => info
            // - "info,a2a_http_server=debug" => info
            // - "a2a_http_server=debug,agent_sdk=info" => (no global) => keep default
            let mut global: Option<String> = None;
            for part in rust_log.split(',') {
                let part = part.trim();
                if part.is_empty() || part.contains('=') {
                    continue;
                }
                // Accept only known levels as a global directive.
                match part.to_ascii_lowercase().as_str() {
                    "error" | "warn" | "info" | "debug" | "trace" => {
                        global = Some(part.to_ascii_lowercase());
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(level) = global {
                cfg.logging.level = level;
            }
        }

        // Logging format: prefer PromptFleet knob, but allow aliases for convenience.
        if let Ok(format) = std::env::var("PF_LOG_FORMAT") {
            cfg.logging.format = format;
        } else if let Ok(format) = std::env::var("OBS_LOG_FORMAT") {
            cfg.logging.format = format;
        } else if let Ok(format) = std::env::var("LOG_FORMAT") {
            cfg.logging.format = format;
        }
        if let Some(v) = env_bool("OBS_LOG_STRUCTURED") {
            cfg.logging.structured = v;
        }
        if let Some(v) = env_bool("OBS_LOG_CONTEXT") {
            cfg.logging.context_enrichment = v;
        }

        #[cfg(feature = "otel")]
        {
            if let Some(enabled) = env_bool("OBS_OTEL_ENABLED") {
                cfg.otel.enabled = enabled;
            } else if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
                || std::env::var("OTEL_SERVICE_NAME").is_ok()
            {
                cfg.otel.enabled = true;
            }
            if let Some(enabled) = env_bool("OBS_OTEL_METRICS_ENABLED") {
                cfg.otel.metrics_enabled = enabled;
            }
            if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
                cfg.otel.otlp_endpoint = endpoint;
            }

            // Sampling controls (dev-friendly).
            //
            // `OBS_OTEL_SAMPLING` accepts:
            // - "always_on" | "on"
            // - "always_off" | "off"
            // - "parent_based"
            // - "ratio:0.1" or "0.1"
            //
            // `OBS_OTEL_SAMPLING_RATIO` is a numeric convenience override.
            if let Ok(sampling) = std::env::var("OBS_OTEL_SAMPLING") {
                let sampling = sampling.trim().to_ascii_lowercase();
                cfg.otel.sampling = match sampling.as_str() {
                    "always_on" | "alwayson" | "on" => OtelSampling::AlwaysOn,
                    "always_off" | "alwaysoff" | "off" => OtelSampling::AlwaysOff,
                    "parent_based" | "parentbased" => OtelSampling::ParentBased,
                    other => {
                        // allow "ratio:0.1" or "0.1"
                        let ratio = other
                            .strip_prefix("ratio:")
                            .unwrap_or(other)
                            .parse::<f64>()
                            .ok();
                        ratio
                            .map(OtelSampling::TraceIdRatio)
                            .unwrap_or(cfg.otel.sampling.clone())
                    }
                };
            } else if let Ok(ratio) = std::env::var("OBS_OTEL_SAMPLING_RATIO") {
                if let Ok(r) = ratio.trim().parse::<f64>() {
                    cfg.otel.sampling = OtelSampling::TraceIdRatio(r);
                }
            }
        }

        #[cfg(feature = "prometheus")]
        {
            if let Some(enabled) = env_bool("OBS_PROMETHEUS_ENABLED") {
                cfg.prometheus.enabled = enabled;
            } else if std::env::var("PROMETHEUS_PUSHGATEWAY").is_ok()
                || std::env::var("PROMETHEUS_JOB_NAME").is_ok()
                || std::env::var("PROMETHEUS_INSTANCE").is_ok()
            {
                cfg.prometheus.enabled = true;
            }

            if let Ok(pg) = std::env::var("PROMETHEUS_PUSHGATEWAY") {
                cfg.prometheus.pushgateway_endpoint = Some(pg);
            }
            if let Ok(job) = std::env::var("PROMETHEUS_JOB_NAME") {
                cfg.prometheus.job_name = job;
            }
            if let Ok(inst) = std::env::var("PROMETHEUS_INSTANCE") {
                cfg.prometheus.instance = inst;
            }
        }

        cfg
    }
}

#[cfg(feature = "config")]
impl ObservabilityConfig {
    /// Build an `ObservabilityConfig` from a JSON value (typically produced by `pf_config`).
    ///
    /// Expected shapes (both accepted):
    /// - `{ "observability": { ... } }`
    /// - `{ ... }` (the observability object directly)
    ///
    /// Missing fields fall back to defaults.
    pub fn from_value(root: &ConfigValue) -> Self {
        fn get_obj<'a>(
            v: &'a ConfigValue,
            key: &str,
        ) -> Option<&'a serde_json::Map<String, ConfigValue>> {
            v.get(key)?.as_object()
        }
        fn get_str(v: &ConfigValue, key: &str) -> Option<String> {
            v.get(key)?.as_str().map(|s| s.to_string())
        }
        fn get_bool(v: &ConfigValue, key: &str) -> Option<bool> {
            v.get(key)?.as_bool()
        }
        fn get_u64(v: &ConfigValue, key: &str) -> Option<u64> {
            v.get(key)?.as_u64()
        }
        fn get_duration_millis(v: &ConfigValue, key: &str) -> Option<Duration> {
            get_u64(v, key).map(Duration::from_millis)
        }
        fn get_usize(v: &ConfigValue, key: &str) -> Option<usize> {
            v.get(key)?.as_u64().map(|n| n as usize)
        }
        fn get_f64(v: &ConfigValue, key: &str) -> Option<f64> {
            v.get(key)?
                .as_f64()
                .or_else(|| v.get(key)?.as_u64().map(|n| n as f64))
        }

        let obs_val = root.get("observability").unwrap_or(root);
        if !obs_val.is_object() {
            return Self::default();
        }

        let mut cfg = Self::default();

        if let Some(s) = get_str(obs_val, "service_name") {
            cfg.service_name = s;
        }
        if let Some(s) = get_str(obs_val, "service_version") {
            cfg.service_version = s;
        }
        if let Some(s) = get_str(obs_val, "service_namespace") {
            cfg.service_namespace = s;
        }
        if let Some(enabled) = get_bool(obs_val, "metrics_enabled") {
            cfg.otel.metrics_enabled = enabled;
        }

        // logging
        if let Some(logging) = get_obj(obs_val, "logging") {
            let logging = ConfigValue::Object(logging.clone());
            if let Some(level) = get_str(&logging, "level") {
                cfg.logging.level = level;
            }
            if let Some(format) = get_str(&logging, "format") {
                cfg.logging.format = format;
            }
            if let Some(structured) = get_bool(&logging, "structured") {
                cfg.logging.structured = structured;
            }
            if let Some(ctx) = get_bool(&logging, "context_enrichment") {
                cfg.logging.context_enrichment = ctx;
            }
        }

        // otel
        if let Some(otel_obj) = get_obj(obs_val, "otel") {
            let otel_val = ConfigValue::Object(otel_obj.clone());
            if let Some(enabled) = get_bool(&otel_val, "enabled") {
                cfg.otel.enabled = enabled;
            }
            if let Some(enabled) = get_bool(&otel_val, "metrics_enabled") {
                cfg.otel.metrics_enabled = enabled;
            }
            if let Some(endpoint) = get_str(&otel_val, "otlp_endpoint") {
                cfg.otel.otlp_endpoint = endpoint;
            }
            if let Some(bs) = get_usize(&otel_val, "batch_size") {
                cfg.otel.batch_size = bs;
            }
            if let Some(timeout) = get_duration_millis(&otel_val, "export_timeout") {
                cfg.otel.export_timeout = timeout;
            } else if let Some(secs) = get_u64(&otel_val, "export_timeout_secs") {
                cfg.otel.export_timeout = Duration::from_secs(secs);
            }
            if let Some(sampling) = get_str(&otel_val, "sampling") {
                cfg.otel.sampling = match sampling.to_ascii_lowercase().as_str() {
                    "always_on" | "alwayson" | "on" => OtelSampling::AlwaysOn,
                    "always_off" | "alwaysoff" | "off" => OtelSampling::AlwaysOff,
                    "parent_based" | "parentbased" => OtelSampling::ParentBased,
                    other => {
                        // allow "ratio:0.1" or "0.1"
                        let ratio = other
                            .strip_prefix("ratio:")
                            .unwrap_or(other)
                            .parse::<f64>()
                            .ok();
                        ratio
                            .map(OtelSampling::TraceIdRatio)
                            .unwrap_or(cfg.otel.sampling.clone())
                    }
                };
            } else if let Some(ratio) = get_f64(&otel_val, "sampling_ratio") {
                cfg.otel.sampling = OtelSampling::TraceIdRatio(ratio);
            }
        }

        // prometheus
        if let Some(p_obj) = get_obj(obs_val, "prometheus") {
            let p_val = ConfigValue::Object(p_obj.clone());
            if let Some(enabled) = get_bool(&p_val, "enabled") {
                cfg.prometheus.enabled = enabled;
            }
            if let Some(pg) = get_str(&p_val, "pushgateway_endpoint") {
                cfg.prometheus.pushgateway_endpoint = Some(pg);
            }
            if let Some(job) = get_str(&p_val, "job_name") {
                cfg.prometheus.job_name = job;
            }
            if let Some(inst) = get_str(&p_val, "instance") {
                cfg.prometheus.instance = inst;
            }
            if let Some(interval) = get_duration_millis(&p_val, "push_interval") {
                cfg.prometheus.push_interval = interval;
            } else if let Some(secs) = get_u64(&p_val, "push_interval_secs") {
                cfg.prometheus.push_interval = Duration::from_secs(secs);
            }
            if let Some(cr) = get_bool(&p_val, "cardinality_reduction") {
                cfg.prometheus.cardinality_reduction = cr;
            }
            if let Some(mc) = get_usize(&p_val, "max_cardinality") {
                cfg.prometheus.max_cardinality = mc;
            }
        }

        cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "config")]
    use serde_json::json;

    #[test]
    fn init_default_config_works() {
        let cfg = ObservabilityConfig::default();
        let obs = Obs::init(cfg);
        assert!(obs.is_ok());
    }

    #[test]
    fn serde_roundtrip_default_config() {
        let cfg = ObservabilityConfig::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: ObservabilityConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg.service_name, back.service_name);
        assert_eq!(cfg.service_version, back.service_version);
        assert_eq!(cfg.otel.enabled, back.otel.enabled);
        assert_eq!(cfg.otel.metrics_enabled, back.otel.metrics_enabled);
        assert_eq!(cfg.prometheus.enabled, back.prometheus.enabled);
    }

    #[test]
    fn serde_roundtrip_full_config() {
        let cfg = ObservabilityConfig {
            service_name: "test-agent".to_string(),
            service_version: "2.0.0".to_string(),
            service_namespace: "prod".to_string(),
            logging: CoreLoggingConfig {
                level: "debug".to_string(),
                format: "json".to_string(),
                structured: true,
                context_enrichment: true,
                default_context: Default::default(),
            },
            otel: OtelConfig {
                enabled: true,
                metrics_enabled: true,
                otlp_endpoint: "http://otel:4317".to_string(),
                batch_size: 256,
                export_timeout: Duration::from_millis(5000),
                sampling: OtelSampling::TraceIdRatio(0.25),
            },
            prometheus: PrometheusConfig {
                enabled: true,
                pushgateway_endpoint: Some("http://pushgw:9091".to_string()),
                job_name: "my-job".to_string(),
                instance: "pod-1".to_string(),
                push_interval: Duration::from_millis(15000),
                cardinality_reduction: false,
                max_cardinality: 5000,
            },
        };
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: ObservabilityConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg.service_name, back.service_name);
        assert_eq!(cfg.otel.enabled, back.otel.enabled);
        assert_eq!(cfg.otel.metrics_enabled, back.otel.metrics_enabled);
        assert_eq!(cfg.otel.otlp_endpoint, back.otel.otlp_endpoint);
        assert_eq!(cfg.otel.batch_size, back.otel.batch_size);
        assert_eq!(cfg.otel.export_timeout, back.otel.export_timeout);
        assert_eq!(cfg.otel.sampling, back.otel.sampling);
        assert_eq!(cfg.prometheus.enabled, back.prometheus.enabled);
        assert_eq!(cfg.prometheus.push_interval, back.prometheus.push_interval);
        assert_eq!(
            cfg.prometheus.max_cardinality,
            back.prometheus.max_cardinality
        );
    }

    #[test]
    fn serde_otel_sampling_variants() {
        let cases = vec![
            (OtelSampling::AlwaysOn, "\"always_on\""),
            (OtelSampling::AlwaysOff, "\"always_off\""),
            (OtelSampling::ParentBased, "\"parent_based\""),
            (OtelSampling::TraceIdRatio(0.5), "\"ratio:0.5\""),
        ];
        for (variant, expected_json) in cases {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected_json, "serialize {:?}", variant);
            let back: OtelSampling = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, back, "roundtrip {:?}", variant);
        }
    }

    #[test]
    fn serde_otel_sampling_aliases() {
        let cases = vec![
            ("\"on\"", OtelSampling::AlwaysOn),
            ("\"off\"", OtelSampling::AlwaysOff),
            ("\"parentbased\"", OtelSampling::ParentBased),
            ("\"0.1\"", OtelSampling::TraceIdRatio(0.1)),
        ];
        for (json, expected) in cases {
            let back: OtelSampling = serde_json::from_str(json).unwrap();
            assert_eq!(expected, back);
        }
    }

    #[test]
    fn serde_duration_millis_roundtrip() {
        let otel = OtelConfig {
            enabled: true,
            metrics_enabled: false,
            otlp_endpoint: "http://localhost:4317".to_string(),
            batch_size: 512,
            export_timeout: Duration::from_millis(7500),
            sampling: OtelSampling::AlwaysOn,
        };
        let json = serde_json::to_string(&otel).unwrap();
        assert!(json.contains("7500"), "Duration should serialize as millis");
        let back: OtelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(otel.export_timeout, back.export_timeout);
        assert!(!back.metrics_enabled);
    }

    #[cfg(feature = "config")]
    #[test]
    fn from_value_roundtrips_canonical_duration_fields() {
        let cfg = ObservabilityConfig {
            service_name: "test-agent".to_string(),
            service_version: "2.0.0".to_string(),
            service_namespace: "prod".to_string(),
            logging: CoreLoggingConfig {
                level: "debug".to_string(),
                format: "json".to_string(),
                structured: true,
                context_enrichment: true,
                default_context: Default::default(),
            },
            otel: OtelConfig {
                enabled: true,
                metrics_enabled: true,
                otlp_endpoint: "http://otel:4317".to_string(),
                batch_size: 256,
                export_timeout: Duration::from_millis(5_500),
                sampling: OtelSampling::ParentBased,
            },
            prometheus: PrometheusConfig {
                enabled: true,
                pushgateway_endpoint: Some("http://pushgw:9091".to_string()),
                job_name: "my-job".to_string(),
                instance: "pod-1".to_string(),
                push_interval: Duration::from_millis(15_250),
                cardinality_reduction: false,
                max_cardinality: 5000,
            },
        };

        let json = serde_json::to_value(&cfg).unwrap();
        let roundtrip: ObservabilityConfig = serde_json::from_value(json.clone()).unwrap();
        let from_value = ObservabilityConfig::from_value(&json);

        assert_eq!(cfg, roundtrip);
        assert_eq!(cfg, from_value);
    }

    #[cfg(feature = "config")]
    #[test]
    fn from_value_prefers_canonical_duration_fields_over_legacy_seconds() {
        let cfg = ObservabilityConfig::from_value(&json!({
            "observability": {
                "service_name": "legacy-bridge",
                "service_version": "1.0.0",
                "service_namespace": "prod",
                "otel": {
                    "enabled": true,
                    "metrics_enabled": false,
                    "otlp_endpoint": "http://otel:4317",
                    "batch_size": 128,
                    "export_timeout": 1500,
                    "export_timeout_secs": 99,
                    "sampling": "always_off"
                },
                "prometheus": {
                    "enabled": true,
                    "pushgateway_endpoint": "http://pushgw:9091",
                    "job_name": "bridge",
                    "instance": "pod-a",
                    "push_interval": 2500,
                    "push_interval_secs": 88
                }
            }
        }));

        assert_eq!(cfg.otel.export_timeout, Duration::from_millis(1500));
        assert!(!cfg.otel.metrics_enabled);
        assert_eq!(cfg.prometheus.push_interval, Duration::from_millis(2500));
        assert!(cfg.otel.enabled);
        assert!(cfg.prometheus.enabled);
    }

    #[cfg(feature = "config")]
    #[test]
    fn from_value_accepts_root_metrics_enabled_for_otel_runtime() {
        let cfg = ObservabilityConfig::from_value(&json!({
            "observability": {
                "metrics_enabled": false,
                "otel": {
                    "enabled": true,
                    "otlp_endpoint": "http://otel:4317"
                }
            }
        }));

        assert!(cfg.otel.enabled);
        assert!(!cfg.otel.metrics_enabled);
    }

    #[test]
    fn health_includes_logging() {
        let obs = Obs::init(ObservabilityConfig::default()).unwrap();
        let health = obs.health();
        assert!(health.logging);
    }

    #[test]
    fn init_is_idempotent() {
        let _a = Obs::init(ObservabilityConfig::default()).unwrap();
        let _b = Obs::init(ObservabilityConfig::default()).unwrap();
    }

    #[test]
    fn w3c_propagation_roundtrip() {
        let ctx = observability_core::W3CTraceContext::new_root();
        let mut headers = HashMap::new();
        Obs::inject_context(&mut headers, &ctx);

        let extracted = Obs::extract_context(&headers)
            .unwrap()
            .expect("expected context");
        assert_eq!(extracted, ctx);
    }

    #[test]
    fn health_marks_failed_backends_when_configured_but_invalid() {
        // This test asserts "best-effort init": invalid backend configs must not panic,
        // and health must surface the failure as `Some(false)` with a helpful note.
        #[cfg(any(feature = "otel", feature = "prometheus"))]
        let mut cfg = ObservabilityConfig::default();
        #[cfg(not(any(feature = "otel", feature = "prometheus")))]
        let cfg = ObservabilityConfig::default();

        #[cfg(feature = "otel")]
        {
            cfg.otel.enabled = true;
            cfg.otel.otlp_endpoint = "".to_string(); // invalid => validate fails => best-effort note
        }

        #[cfg(feature = "prometheus")]
        {
            cfg.prometheus.enabled = true;
            cfg.prometheus.job_name = "".to_string(); // invalid => validate fails => best-effort note
        }

        let obs = Obs::init(cfg).unwrap();
        let health = obs.health();

        #[cfg(feature = "otel")]
        assert_eq!(health.otel, Some(false));
        #[cfg(feature = "prometheus")]
        assert_eq!(health.prometheus, Some(false));

        // Only assert init failure notes when at least one optional backend is compiled in.
        // With no backends enabled, init can legitimately have no notes.
        if cfg!(feature = "otel") || cfg!(feature = "prometheus") {
            assert!(
                !health.notes.is_empty(),
                "expected health notes to contain init failure reason(s)"
            );
        }
    }
}
