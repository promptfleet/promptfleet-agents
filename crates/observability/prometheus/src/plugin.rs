//! Main Prometheus 2025 plugin implementation

use observability_core::{
    ports::MetricsPort,
    traits::{SpanGuard, SpanStatus, METRIC_LABEL_ALLOWLIST},
    ObservabilityError, ObservabilityPlugin, ObservabilityResult,
};

#[cfg(feature = "cardinality-reduction")]
use crate::cardinality_reduction::CardinalityReducer;
#[cfg(feature = "pushgateway-client")]
use crate::pushgateway_client::PushGatewayClient;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use web_time::{Duration, Instant};

#[cfg(feature = "prometheus-federation")]
use prometheus_crate::{CounterVec, GaugeVec, HistogramVec, Registry};

#[cfg(feature = "prometheus-federation")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MetricFamilyKind {
    Counter,
    Histogram,
    Gauge,
}

/// Configuration for Prometheus extension
#[derive(Debug, Clone)]
pub struct PrometheusConfig {
    /// Push gateway endpoint (optional)
    pub pushgateway_endpoint: Option<String>,
    /// Job name for push gateway
    pub job_name: String,
    /// Instance identifier
    pub instance: String,
    /// Push interval for batch metrics
    pub push_interval: Duration,
    /// Enable cardinality reduction
    pub cardinality_reduction: bool,
    /// Maximum number of unique label combinations
    pub max_cardinality: usize,
    /// Enable hierarchical federation
    pub hierarchical_federation: bool,
    /// Additional labels for all metrics
    pub global_labels: HashMap<String, String>,
}

impl Default for PrometheusConfig {
    fn default() -> Self {
        Self {
            pushgateway_endpoint: Some(crate::DEFAULT_PUSHGATEWAY_ENDPOINT.to_string()),
            job_name: "spinkube-agent".to_string(),
            instance: "localhost:9090".to_string(),
            push_interval: Duration::from_secs(crate::DEFAULT_PUSH_INTERVAL_SECS),
            cardinality_reduction: true,
            max_cardinality: 10000,
            hierarchical_federation: true,
            global_labels: HashMap::new(),
        }
    }
}

impl PrometheusConfig {
    /// Create a new configuration builder
    pub fn builder() -> PrometheusConfigBuilder {
        PrometheusConfigBuilder::new()
    }

    /// Create from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(endpoint) = std::env::var("PROMETHEUS_PUSHGATEWAY") {
            config.pushgateway_endpoint = Some(endpoint);
        }

        if let Ok(job_name) = std::env::var("PROMETHEUS_JOB_NAME") {
            config.job_name = job_name;
        }

        if let Ok(instance) = std::env::var("PROMETHEUS_INSTANCE") {
            config.instance = instance;
        }

        // Parse global labels from environment
        if let Ok(labels) = std::env::var("PROMETHEUS_GLOBAL_LABELS") {
            for label in labels.split(',') {
                if let Some((key, value)) = label.split_once('=') {
                    config
                        .global_labels
                        .insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }

        config
    }
}

/// Builder for Prometheus configuration
pub struct PrometheusConfigBuilder {
    config: PrometheusConfig,
}

impl PrometheusConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: PrometheusConfig::default(),
        }
    }

    pub fn with_pushgateway(mut self, endpoint: impl Into<String>) -> Self {
        self.config.pushgateway_endpoint = Some(endpoint.into());
        self
    }

    pub fn with_job_name(mut self, job_name: impl Into<String>) -> Self {
        self.config.job_name = job_name.into();
        self
    }

    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.config.instance = instance.into();
        self
    }

    pub fn with_push_interval(mut self, interval: Duration) -> Self {
        self.config.push_interval = interval;
        self
    }

    pub fn with_cardinality_reduction(mut self, enabled: bool) -> Self {
        self.config.cardinality_reduction = enabled;
        self
    }

    pub fn with_max_cardinality(mut self, max: usize) -> Self {
        self.config.max_cardinality = max;
        self
    }

    pub fn with_global_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.global_labels.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> PrometheusConfig {
        self.config
    }
}

/// Prometheus extension with modern best practices
#[cfg(feature = "prometheus-federation")]
pub struct Prometheus {
    config: PrometheusConfig,
    registry: Arc<Registry>,
    #[cfg(feature = "cardinality-reduction")]
    cardinality_reducer: Arc<Mutex<CardinalityReducer>>,
    #[cfg(feature = "pushgateway-client")]
    push_client: Option<Arc<PushGatewayClient>>,
    metrics: Arc<Mutex<PluginMetrics>>,
    last_push: Arc<Mutex<Instant>>,
}

#[cfg(feature = "prometheus-federation")]
struct PluginMetrics {
    // Core metrics for observability
    span_duration: HistogramVec,
    span_count: CounterVec,
    metric_count: CounterVec,
    log_count: CounterVec,
    // Custom metrics registry
    custom_counters: HashMap<String, CounterVec>,
    custom_histograms: HashMap<String, HistogramVec>,
    custom_gauges: HashMap<String, GaugeVec>,
}

#[cfg(feature = "prometheus-federation")]
impl Prometheus {
    fn metric_family_kind(name: &str) -> MetricFamilyKind {
        if name.ends_with("_total") || name.ends_with("_count") || name.ends_with("_counter") {
            MetricFamilyKind::Counter
        } else if name.contains("_latency")
            || name.contains("_duration")
            || name.ends_with("_ms")
            || name.ends_with("_seconds")
        {
            MetricFamilyKind::Histogram
        } else {
            MetricFamilyKind::Gauge
        }
    }

    fn metric_help(name: &str, kind: MetricFamilyKind) -> String {
        match kind {
            MetricFamilyKind::Counter => format!("Counter metric: {}", name),
            MetricFamilyKind::Histogram => format!("Histogram metric: {}", name),
            MetricFamilyKind::Gauge => format!("Gauge metric: {}", name),
        }
    }

    fn label_values_for_allowlist<'a>(labels: &'a HashMap<String, String>) -> Vec<&'a str> {
        METRIC_LABEL_ALLOWLIST
            .iter()
            .map(|&label| labels.get(label).map(|s| s.as_str()).unwrap_or("unknown"))
            .collect()
    }

    /// Create a new Prometheus extension with configuration
    pub fn new(config: PrometheusConfig) -> ObservabilityResult<Self> {
        let registry = Arc::new(Registry::new());

        // Initialize core metrics
        let span_duration = HistogramVec::new(
            prometheus_crate::HistogramOpts::new(
                "span_duration_seconds",
                "Duration of spans in seconds",
            )
            .buckets(vec![0.001, 0.01, 0.1, 1.0, 10.0]),
            METRIC_LABEL_ALLOWLIST,
        )
        .map_err(|e| {
            ObservabilityError::metric(format!("Failed to create span_duration metric: {}", e))
        })?;

        let span_count = CounterVec::new(
            prometheus_crate::Opts::new("span_total", "Total number of spans"),
            METRIC_LABEL_ALLOWLIST,
        )
        .map_err(|e| {
            ObservabilityError::metric(format!("Failed to create span_count metric: {}", e))
        })?;

        let metric_count = CounterVec::new(
            prometheus_crate::Opts::new("metric_total", "Total number of metrics recorded"),
            METRIC_LABEL_ALLOWLIST,
        )
        .map_err(|e| {
            ObservabilityError::metric(format!("Failed to create metric_count metric: {}", e))
        })?;

        let log_count = CounterVec::new(
            prometheus_crate::Opts::new("log_total", "Total number of log messages"),
            &["level", "component"],
        )
        .map_err(|e| {
            ObservabilityError::metric(format!("Failed to create log_count metric: {}", e))
        })?;

        // Register metrics
        registry
            .register(Box::new(span_duration.clone()))
            .map_err(|e| {
                ObservabilityError::metric(format!("Failed to register span_duration: {}", e))
            })?;
        registry
            .register(Box::new(span_count.clone()))
            .map_err(|e| {
                ObservabilityError::metric(format!("Failed to register span_count: {}", e))
            })?;
        registry
            .register(Box::new(metric_count.clone()))
            .map_err(|e| {
                ObservabilityError::metric(format!("Failed to register metric_count: {}", e))
            })?;
        registry
            .register(Box::new(log_count.clone()))
            .map_err(|e| {
                ObservabilityError::metric(format!("Failed to register log_count: {}", e))
            })?;

        let metrics = Arc::new(Mutex::new(PluginMetrics {
            span_duration,
            span_count,
            metric_count,
            log_count,
            custom_counters: HashMap::new(),
            custom_histograms: HashMap::new(),
            custom_gauges: HashMap::new(),
        }));

        #[cfg(feature = "cardinality-reduction")]
        let cardinality_reducer =
            Arc::new(Mutex::new(CardinalityReducer::new(config.max_cardinality)));

        #[cfg(feature = "pushgateway-client")]
        let push_client = if let Some(endpoint) = &config.pushgateway_endpoint {
            Some(Arc::new(PushGatewayClient::new(endpoint)?))
        } else {
            None
        };

        Ok(Self {
            config,
            registry,
            #[cfg(feature = "cardinality-reduction")]
            cardinality_reducer,
            #[cfg(feature = "pushgateway-client")]
            push_client,
            metrics,
            last_push: Arc::new(Mutex::new(Instant::now())),
        })
    }

    /// Create from environment variables
    pub fn from_env() -> ObservabilityResult<Self> {
        let config = PrometheusConfig::from_env();
        Self::new(config)
    }

    /// Create with default configuration
    pub fn default() -> ObservabilityResult<Self> {
        Self::new(PrometheusConfig::default())
    }

    /// Get the Prometheus registry for custom metrics
    pub fn registry(&self) -> Arc<Registry> {
        self.registry.clone()
    }

    /// Add a custom counter metric
    pub fn add_counter(&self, name: &str, help: &str, labels: &[&str]) -> ObservabilityResult<()> {
        let counter =
            CounterVec::new(prometheus_crate::Opts::new(name, help), labels).map_err(|e| {
                ObservabilityError::metric(format!("Failed to create counter {}: {}", name, e))
            })?;

        self.registry
            .register(Box::new(counter.clone()))
            .map_err(|e| {
                ObservabilityError::metric(format!("Failed to register counter {}: {}", name, e))
            })?;

        let mut metrics = self.metrics.lock().unwrap();
        metrics.custom_counters.insert(name.to_string(), counter);
        Ok(())
    }

    /// Record a metric value (sync version for sync interface)
    pub fn record_metric_with_labels(
        &self,
        name: &str,
        value: f64,
        labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()> {
        #[cfg(feature = "cardinality-reduction")]
        {
            let mut reducer = self.cardinality_reducer.lock().unwrap();
            if !reducer.should_record(name, labels) {
                return Ok(()); // Skip this metric to reduce cardinality
            }
        }

        let label_values = Self::label_values_for_allowlist(labels);
        let metric_kind = Self::metric_family_kind(name);

        {
            let mut metrics = self.metrics.lock().unwrap();

            match metric_kind {
                MetricFamilyKind::Counter => {
                    if !metrics.custom_counters.contains_key(name) {
                        let counter = CounterVec::new(
                            prometheus_crate::Opts::new(
                                name,
                                &Self::metric_help(name, metric_kind),
                            ),
                            METRIC_LABEL_ALLOWLIST,
                        )
                        .map_err(|e| {
                            ObservabilityError::metric(format!(
                                "Failed to create counter {}: {}",
                                name, e
                            ))
                        })?;

                        self.registry
                            .register(Box::new(counter.clone()))
                            .map_err(|e| {
                                ObservabilityError::metric(format!(
                                    "Failed to register counter {}: {}",
                                    name, e
                                ))
                            })?;
                        metrics.custom_counters.insert(name.to_string(), counter);
                    }

                    if let Some(counter) = metrics.custom_counters.get(name) {
                        counter.with_label_values(&label_values).inc_by(value);
                    }
                }
                MetricFamilyKind::Histogram => {
                    if !metrics.custom_histograms.contains_key(name) {
                        let histogram = HistogramVec::new(
                            prometheus_crate::HistogramOpts::new(
                                name,
                                &Self::metric_help(name, metric_kind),
                            )
                            .buckets(vec![0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0]),
                            METRIC_LABEL_ALLOWLIST,
                        )
                        .map_err(|e| {
                            ObservabilityError::metric(format!(
                                "Failed to create histogram {}: {}",
                                name, e
                            ))
                        })?;

                        self.registry
                            .register(Box::new(histogram.clone()))
                            .map_err(|e| {
                                ObservabilityError::metric(format!(
                                    "Failed to register histogram {}: {}",
                                    name, e
                                ))
                            })?;
                        metrics
                            .custom_histograms
                            .insert(name.to_string(), histogram);
                    }

                    if let Some(histogram) = metrics.custom_histograms.get(name) {
                        histogram.with_label_values(&label_values).observe(value);
                    }
                }
                MetricFamilyKind::Gauge => {
                    if !metrics.custom_gauges.contains_key(name) {
                        let gauge = GaugeVec::new(
                            prometheus_crate::Opts::new(
                                name,
                                &Self::metric_help(name, metric_kind),
                            ),
                            METRIC_LABEL_ALLOWLIST,
                        )
                        .map_err(|e| {
                            ObservabilityError::metric(format!(
                                "Failed to create gauge {}: {}",
                                name, e
                            ))
                        })?;

                        self.registry
                            .register(Box::new(gauge.clone()))
                            .map_err(|e| {
                                ObservabilityError::metric(format!(
                                    "Failed to register gauge {}: {}",
                                    name, e
                                ))
                            })?;
                        metrics.custom_gauges.insert(name.to_string(), gauge);
                    }

                    if let Some(gauge) = metrics.custom_gauges.get(name) {
                        gauge.with_label_values(&label_values).set(value);
                    }
                }
            }

            metrics.metric_count.with_label_values(&label_values).inc();
        }

        // Check if we should push metrics (schedule async push)
        self.check_and_schedule_push()?;
        Ok(())
    }

    /// Check if metrics should be pushed and schedule if necessary (non-blocking)
    fn check_and_schedule_push(&self) -> ObservabilityResult<()> {
        #[cfg(feature = "pushgateway-client")]
        {
            let should_push = {
                let last_push = self.last_push.lock().unwrap();
                last_push.elapsed() >= self.config.push_interval
            };

            if should_push {
                if let Some(_push_client) = &self.push_client {
                    // Schedule async push - in a real implementation, you'd use a background task
                    // For now, we'll just update the timestamp to avoid continuous pushing
                    let mut last_push = self.last_push.lock().unwrap();
                    *last_push = Instant::now();
                }
            }
        }
        Ok(())
    }

    /// Force push all metrics to push gateway (async for SpinKube)
    pub async fn force_push(&self) -> ObservabilityResult<()> {
        #[cfg(feature = "pushgateway-client")]
        {
            if let Some(push_client) = &self.push_client {
                push_client
                    .push_metrics(&self.registry, &self.config.job_name, &self.config.instance)
                    .await?;

                let mut last_push = self.last_push.lock().unwrap();
                *last_push = Instant::now();
            }
        }
        Ok(())
    }

    /// Async-safe flush — safe to call from within tokio or any async runtime.
    pub async fn flush_async(&self) -> ObservabilityResult<()> {
        self.force_push().await
    }
}

impl Prometheus {
    /// Record span completion with a nominal duration.
    ///
    /// Full duration tracking requires span-state storage (OTel plugin handles that).
    /// Here we just record that a span ended so counters stay consistent.
    fn record_span_end(&self, labels: &HashMap<String, String>) {
        let label_values: Vec<&str> = METRIC_LABEL_ALLOWLIST
            .iter()
            .map(|&label| labels.get(label).map(|s| s.as_str()).unwrap_or("unknown"))
            .collect();

        if let Ok(metrics) = self.metrics.lock() {
            metrics
                .span_duration
                .with_label_values(&label_values)
                .observe(0.001);
        }
    }

    /// Build a full label-values vector from partial overrides, filling
    /// missing positions with `"unknown"`.
    fn allowlist_values<'a>(overrides: &[(&str, &'a str)]) -> Vec<&'a str> {
        METRIC_LABEL_ALLOWLIST
            .iter()
            .map(|&label| {
                overrides
                    .iter()
                    .find(|(k, _)| *k == label)
                    .map(|(_, v)| *v)
                    .unwrap_or("unknown")
            })
            .collect()
    }
}

#[cfg(feature = "prometheus-federation")]
impl ObservabilityPlugin for Prometheus {
    fn start_span(&self, _name: &str, attributes: &[(&str, &str)]) -> SpanGuard {
        let labels: HashMap<String, String> = attributes
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let label_values: Vec<&str> = METRIC_LABEL_ALLOWLIST
            .iter()
            .map(|&label| labels.get(label).map(|s| s.as_str()).unwrap_or("unknown"))
            .collect();

        {
            let metrics = self.metrics.lock().unwrap();
            metrics.span_count.with_label_values(&label_values).inc();
        }

        #[cfg(feature = "uuid")]
        let span_id = uuid::Uuid::new_v4().to_string();
        #[cfg(not(feature = "uuid"))]
        let span_id = format!("span_{}", std::ptr::addr_of!(*self) as usize);

        SpanGuard::new(
            span_id,
            Arc::new(self.clone()) as Arc<dyn ObservabilityPlugin>,
        )
    }

    fn end_span(&self, _span_id: &str) {
        let labels = HashMap::new();
        self.record_span_end(&labels);
    }

    fn add_span_attribute(&self, _span_id: &str, _key: &str, _value: &str) {
        // Prometheus doesn't support dynamic span attributes
    }

    fn set_span_status(&self, _span_id: &str, _status: SpanStatus) {
        // Prometheus doesn't track span status directly
    }

    fn record_metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let label_map: HashMap<String, String> = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Call sync version - errors are logged but don't fail
        if let Err(e) = self.record_metric_with_labels(name, value, &label_map) {
            // Log error but don't fail - metrics should be best effort
            eprintln!("Failed to record metric {}: {}", name, e);
        }
    }

    #[cfg(feature = "structured-logging")]
    fn log_structured(
        &self,
        level: observability_core::traits::LogLevel,
        _message: &str,
        _fields: &serde_json::Value,
    ) {
        // Count log messages by level
        let metrics = self.metrics.lock().unwrap();
        metrics
            .log_count
            .with_label_values(&[level.as_str(), "prometheus"])
            .inc();
    }

    fn write_log(&self, _message: &str) {
        // Count generic log messages
        let metrics = self.metrics.lock().unwrap();
        metrics
            .log_count
            .with_label_values(&["info", "prometheus"])
            .inc();
    }

    /// # Deadlock hazard
    ///
    /// This calls `futures::executor::block_on` internally. **Do not** call from
    /// inside a tokio (or other async) runtime — it will deadlock.
    /// Use [`Prometheus::flush_async`] instead when running in an async context.
    fn flush(&self) -> ObservabilityResult<()> {
        futures::executor::block_on(async { self.force_push().await })
    }

    fn is_enabled(&self) -> bool {
        true
    }

    fn plugin_type(&self) -> &'static str {
        "prometheus-2025"
    }
}

// Clone implementation for SpanGuard usage
#[cfg(feature = "prometheus-federation")]
impl Clone for Prometheus {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            registry: self.registry.clone(),
            #[cfg(feature = "cardinality-reduction")]
            cardinality_reducer: self.cardinality_reducer.clone(),
            #[cfg(feature = "pushgateway-client")]
            push_client: self.push_client.clone(),
            metrics: self.metrics.clone(),
            last_push: self.last_push.clone(),
        }
    }
}

#[cfg(feature = "prometheus-federation")]
impl MetricsPort for Prometheus {
    fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|e| ObservabilityError::metric(format!("Failed to lock metrics: {}", e)))?;

        let counter = metrics
            .custom_counters
            .entry(name.to_string())
            .or_insert_with(|| {
                CounterVec::new(
                    prometheus_crate::Opts::new(name, &format!("Counter metric: {}", name)),
                    &["component", "operation"],
                )
                .unwrap()
            });

        counter
            .with_label_values(&["prometheus_plugin", "metric_emission"])
            .inc_by(value);

        let vals = Self::allowlist_values(&[
            ("component", "prometheus_plugin"),
            ("operation", "emit_counter"),
        ]);
        metrics.metric_count.with_label_values(&vals).inc();

        drop(metrics);
        self.check_and_schedule_push()?;

        Ok(())
    }

    fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|e| ObservabilityError::metric(format!("Failed to lock metrics: {}", e)))?;

        let histogram = metrics
            .custom_histograms
            .entry(name.to_string())
            .or_insert_with(|| {
                HistogramVec::new(
                    prometheus_crate::HistogramOpts::new(
                        name,
                        &format!("Histogram metric: {}", name),
                    )
                    .buckets(vec![0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0]),
                    &["component", "operation"],
                )
                .unwrap()
            });

        histogram
            .with_label_values(&["prometheus_plugin", "metric_emission"])
            .observe(value);

        let vals = Self::allowlist_values(&[
            ("component", "prometheus_plugin"),
            ("operation", "emit_histogram"),
        ]);
        metrics.metric_count.with_label_values(&vals).inc();

        drop(metrics);
        self.check_and_schedule_push()?;

        Ok(())
    }

    fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let mut metrics = self
            .metrics
            .lock()
            .map_err(|e| ObservabilityError::metric(format!("Failed to lock metrics: {}", e)))?;

        let gauge = metrics
            .custom_gauges
            .entry(name.to_string())
            .or_insert_with(|| {
                GaugeVec::new(
                    prometheus_crate::Opts::new(name, &format!("Gauge metric: {}", name)),
                    &["component", "operation"],
                )
                .unwrap()
            });

        gauge
            .with_label_values(&["prometheus_plugin", "metric_emission"])
            .set(value);

        let vals = Self::allowlist_values(&[
            ("component", "prometheus_plugin"),
            ("operation", "emit_gauge"),
        ]);
        metrics.metric_count.with_label_values(&vals).inc();

        drop(metrics);
        self.check_and_schedule_push()?;

        Ok(())
    }

    /// Check if metrics collection is enabled
    fn is_enabled(&self) -> bool {
        true // Always enabled for Prometheus plugin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "prometheus-federation")]
    fn test_config_builder() {
        let config = PrometheusConfig::builder()
            .with_job_name("test-job")
            .with_pushgateway("http://localhost:9091")
            .with_cardinality_reduction(true)
            .with_global_label("environment", "test")
            .build();

        assert_eq!(config.job_name, "test-job");
        assert_eq!(
            config.pushgateway_endpoint,
            Some("http://localhost:9091".to_string())
        );
        assert!(config.cardinality_reduction);
        assert_eq!(
            config.global_labels.get("environment"),
            Some(&"test".to_string())
        );
    }

    #[test]
    #[cfg(feature = "prometheus-federation")]
    fn test_plugin_creation() {
        let config = PrometheusConfig::default();
        let plugin = Prometheus::new(config);
        assert!(plugin.is_ok());
    }

    #[test]
    #[cfg(feature = "prometheus-federation")]
    fn test_record_metric_with_labels_routes_by_name_and_keeps_allowlist() {
        use prometheus_crate::proto::MetricType;

        let plugin = Prometheus::new(PrometheusConfig::default()).expect("plugin");

        let counter_labels = HashMap::from([
            ("component".to_string(), "sdk".to_string()),
            ("operation".to_string(), "requests".to_string()),
            ("extra".to_string(), "ignored".to_string()),
        ]);
        plugin
            .record_metric_with_labels("requests_total", 3.0, &counter_labels)
            .expect("counter record");

        let histogram_labels = HashMap::from([
            ("component".to_string(), "sdk".to_string()),
            ("operation".to_string(), "latency".to_string()),
        ]);
        plugin
            .record_metric_with_labels("request_duration_seconds", 1.5, &histogram_labels)
            .expect("histogram record");

        let gauge_labels = HashMap::from([
            ("component".to_string(), "sdk".to_string()),
            ("operation".to_string(), "inflight".to_string()),
        ]);
        plugin
            .record_metric_with_labels("inflight_requests", 7.0, &gauge_labels)
            .expect("gauge record");

        let families = plugin.registry().gather();

        let counter_family = families
            .iter()
            .find(|family| family.get_name() == "requests_total")
            .expect("counter family");
        assert_eq!(counter_family.get_field_type(), MetricType::COUNTER);
        assert_eq!(counter_family.get_metric().len(), 1);
        let counter_metric = &counter_family.get_metric()[0];
        assert_eq!(counter_metric.get_counter().get_value(), 3.0);
        assert!(counter_metric
            .get_label()
            .iter()
            .any(|label| label.get_name() == "component" && label.get_value() == "sdk"));
        assert!(!counter_metric
            .get_label()
            .iter()
            .any(|label| label.get_name() == "extra"));

        let histogram_family = families
            .iter()
            .find(|family| family.get_name() == "request_duration_seconds")
            .expect("histogram family");
        assert_eq!(histogram_family.get_field_type(), MetricType::HISTOGRAM);
        assert_eq!(
            histogram_family.get_metric()[0]
                .get_histogram()
                .get_sample_count(),
            1
        );

        let gauge_family = families
            .iter()
            .find(|family| family.get_name() == "inflight_requests")
            .expect("gauge family");
        assert_eq!(gauge_family.get_field_type(), MetricType::GAUGE);
        assert_eq!(gauge_family.get_metric()[0].get_gauge().get_value(), 7.0);
    }
}
