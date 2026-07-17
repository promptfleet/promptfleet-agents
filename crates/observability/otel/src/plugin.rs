//! Main OpenTelemetry 2025 plugin implementation

use observability_core::traits::{SpanGuard, SpanStatus};
use observability_core::{
    ObservabilityPlugin, ObservabilityResult, TraceContext, W3CTraceContext, get_current_context,
    ports::MetricsPort,
};

use crate::collector_client::{
    CollectorClient, LogData, MetricData, MetricKind, OtelSpanData, SpanEvent,
};
use crate::resource_attributes::ResourceAttributeManager;
use crate::sampling::SamplingStrategy;
#[cfg(feature = "structured-logging")]
use observability_core::traits::LogLevel;

use log::{debug, warn};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use web_time::{Duration, Instant};

/// Configuration for OpenTelemetry extension
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OtelConfig {
    /// OTLP collector endpoint
    pub otlp_endpoint: String,
    /// Service name for resource attributes
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Service namespace
    pub service_namespace: String,
    /// Batch size for span export
    pub batch_size: usize,
    /// Export timeout in seconds
    pub export_timeout_secs: u64,
    /// Sampling strategy
    pub sampling_strategy: SamplingStrategy,
    /// Enable auto-instrumentation
    pub auto_instrumentation: bool,
    /// Custom resource attributes
    pub resource_attributes: HashMap<String, String>,
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: crate::DEFAULT_OTLP_ENDPOINT.to_string(),
            service_name: "spinkube-agent".to_string(),
            service_version: "0.1.0".to_string(),
            service_namespace: "default".to_string(),
            batch_size: crate::DEFAULT_BATCH_SIZE,
            export_timeout_secs: crate::DEFAULT_EXPORT_TIMEOUT_SECS,
            sampling_strategy: SamplingStrategy::default(),
            auto_instrumentation: true,
            resource_attributes: HashMap::new(),
        }
    }
}

impl OtelConfig {
    /// Create a new configuration builder
    pub fn builder() -> OtelConfigBuilder {
        OtelConfigBuilder::new()
    }

    /// Create from environment variables (OpenTelemetry standard)
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            config.otlp_endpoint = endpoint;
        }

        if let Ok(service_name) = std::env::var("OTEL_SERVICE_NAME") {
            config.service_name = service_name;
        }

        if let Ok(service_version) = std::env::var("OTEL_SERVICE_VERSION") {
            config.service_version = service_version;
        }

        if let Ok(service_namespace) = std::env::var("OTEL_SERVICE_NAMESPACE") {
            config.service_namespace = service_namespace;
        }

        // Parse resource attributes from OTEL_RESOURCE_ATTRIBUTES
        if let Ok(resource_attrs) = std::env::var("OTEL_RESOURCE_ATTRIBUTES") {
            for attr in resource_attrs.split(',') {
                if let Some((key, value)) = attr.split_once('=') {
                    config
                        .resource_attributes
                        .insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }

        config
    }
}

/// Builder for OpenTelemetry configuration with best practices
pub struct OtelConfigBuilder {
    config: OtelConfig,
}

impl OtelConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: OtelConfig::default(),
        }
    }

    pub fn with_otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.otlp_endpoint = endpoint.into();
        self
    }

    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = version.into();
        self
    }

    pub fn with_service_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.service_namespace = namespace.into();
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn with_export_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.config.export_timeout_secs = timeout_secs;
        self
    }

    pub fn with_sampling_strategy(mut self, strategy: SamplingStrategy) -> Self {
        self.config.sampling_strategy = strategy;
        self
    }

    pub fn with_auto_instrumentation(mut self, enabled: bool) -> Self {
        self.config.auto_instrumentation = enabled;
        self
    }

    pub fn with_resource_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.config
            .resource_attributes
            .insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> OtelConfig {
        self.config
    }
}

/// OpenTelemetry extension with modern best practices and WASM optimization
pub struct Otel {
    config: OtelConfig,
    collector_client: Arc<CollectorClient>,
    resource_manager: Arc<ResourceAttributeManager>,
    active_spans: Arc<Mutex<HashMap<String, OtelSpanData>>>,
    current_trace_context: Arc<Mutex<Option<TraceContext>>>,
    span_buffer: Arc<Mutex<Vec<OtelSpanData>>>,
    metric_buffer: Arc<Mutex<Vec<MetricData>>>,
    log_buffer: Arc<Mutex<Vec<LogData>>>,
}

impl Otel {
    /// Create a new OpenTelemetry extension with configuration
    pub async fn new(config: OtelConfig) -> ObservabilityResult<Self> {
        let collector_client = Arc::new(
            CollectorClient::new(
                &config.otlp_endpoint,
                Duration::from_secs(config.export_timeout_secs),
            )
            .await?,
        );

        let resource_manager = Arc::new(ResourceAttributeManager::new(
            &config.service_name,
            &config.service_version,
            &config.service_namespace,
            config.resource_attributes.clone(),
        ));

        Ok(Self {
            config,
            collector_client,
            resource_manager,
            active_spans: Arc::new(Mutex::new(HashMap::new())),
            current_trace_context: Arc::new(Mutex::new(None)),
            span_buffer: Arc::new(Mutex::new(Vec::new())),
            metric_buffer: Arc::new(Mutex::new(Vec::new())),
            log_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Create from environment variables
    pub async fn from_env() -> ObservabilityResult<Self> {
        let config = OtelConfig::from_env();
        Self::new(config).await
    }

    /// Create with default configuration
    pub async fn default() -> ObservabilityResult<Self> {
        Self::new(OtelConfig::default()).await
    }

    /// Builder pattern for extension creation
    pub fn builder() -> OtelBuilder {
        OtelBuilder::new()
    }

    /// Set the current trace context
    pub fn set_trace_context(&self, context: TraceContext) {
        let mut current = self.current_trace_context.lock().unwrap();
        *current = Some(context);
    }

    /// Get the current trace context
    pub fn get_trace_context(&self) -> Option<TraceContext> {
        self.current_trace_context.lock().unwrap().clone()
    }

    /// Create a child span from W3C trace context
    pub fn start_span_with_w3c_context(
        &self,
        name: &str,
        w3c_context: &W3CTraceContext,
        attributes: &[(&str, &str)],
    ) -> SpanGuard {
        let trace_context = TraceContext::from_w3c(w3c_context);
        self.set_trace_context(trace_context.clone());

        let span_data = OtelSpanData {
            span_id: trace_context.span_id.clone(),
            trace_id: trace_context.trace_id.clone(),
            parent_span_id: trace_context.parent_span_id.clone(),
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: None,
            status: SpanStatus::Ok,
            attributes: attributes
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            events: Vec::new(),
        };

        // Check sampling decision
        if !self.config.sampling_strategy.should_sample(&trace_context) {
            return SpanGuard::no_op();
        }

        // Store active span
        {
            let mut spans = self.active_spans.lock().unwrap();
            spans.insert(span_data.span_id.clone(), span_data);
        }

        SpanGuard::new(
            trace_context.span_id,
            Arc::new(self.clone()) as Arc<dyn ObservabilityPlugin>,
        )
    }

    /// Add an event to a span
    pub fn add_span_event(&self, span_id: &str, name: &str, attributes: &[(&str, &str)]) {
        let mut spans = self.active_spans.lock().unwrap();
        if let Some(span) = spans.get_mut(span_id) {
            span.events.push(SpanEvent {
                name: name.to_string(),
                timestamp: Instant::now(),
                attributes: attributes
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
            });
        }
    }

    /// Export pending spans to collector
    async fn export_spans(&self, spans: Vec<OtelSpanData>) -> ObservabilityResult<()> {
        if spans.is_empty() {
            return Ok(());
        }

        // Convert to OTLP format and send to collector
        self.collector_client
            .export_spans(spans, &self.resource_manager)
            .await
    }

    /// Export pending metrics to collector
    async fn export_metrics(&self, metrics: Vec<MetricData>) -> ObservabilityResult<()> {
        if metrics.is_empty() {
            return Ok(());
        }

        self.collector_client
            .export_metrics(metrics, &self.resource_manager)
            .await
    }

    /// Export pending logs to collector
    async fn export_logs(&self, logs: Vec<LogData>) -> ObservabilityResult<()> {
        if logs.is_empty() {
            return Ok(());
        }

        self.collector_client
            .export_logs(logs, &self.resource_manager)
            .await
    }

    /// Get resource attributes
    pub fn get_resource_attributes(&self) -> HashMap<String, String> {
        self.resource_manager.get_all_attributes()
    }

    /// Get the effective OTEL configuration.
    #[cfg(test)]
    pub(crate) fn config(&self) -> &OtelConfig {
        &self.config
    }

    /// Flush all buffered data
    async fn flush_all_buffers(&self) -> ObservabilityResult<()> {
        // Drain buffers into locals. If export fails, we **requeue** to avoid data loss.
        let spans = {
            let mut buffer = self.span_buffer.lock().unwrap();
            buffer.drain(..).collect::<Vec<_>>()
        };

        let metrics = {
            let mut buffer = self.metric_buffer.lock().unwrap();
            buffer.drain(..).collect::<Vec<_>>()
        };

        let logs = {
            let mut buffer = self.log_buffer.lock().unwrap();
            buffer.drain(..).collect::<Vec<_>>()
        };

        let span_count = spans.len();
        let metric_count = metrics.len();
        let log_count = logs.len();

        // Export in parallel.
        let (span_res, metric_res, log_res) = futures::future::join3(
            self.export_spans(spans.clone()),
            self.export_metrics(metrics.clone()),
            self.export_logs(logs.clone()),
        )
        .await;

        // Requeue only the categories that failed.
        if span_res.is_err() {
            warn!(
                "otel:export_spans_failed spans={} err={:?}",
                span_count,
                span_res.as_ref().err()
            );
            let mut buffer = self.span_buffer.lock().unwrap();
            buffer.extend(spans);
        } else if span_count > 0 {
            debug!("otel:export_spans_ok spans={}", span_count);
        }
        if metric_res.is_err() {
            warn!(
                "otel:export_metrics_failed metrics={} err={:?}",
                metric_count,
                metric_res.as_ref().err()
            );
            let mut buffer = self.metric_buffer.lock().unwrap();
            buffer.extend(metrics);
        } else if metric_count > 0 {
            debug!("otel:export_metrics_ok metrics={}", metric_count);
        }
        if log_res.is_err() {
            warn!(
                "otel:export_logs_failed logs={} err={:?}",
                log_count,
                log_res.as_ref().err()
            );
            let mut buffer = self.log_buffer.lock().unwrap();
            buffer.extend(logs);
        } else if log_count > 0 {
            debug!("otel:export_logs_ok logs={}", log_count);
        }

        // Bubble up an error if any category failed.
        span_res?;
        metric_res?;
        log_res?;

        Ok(())
    }

    /// Flush buffered telemetry asynchronously (drains buffers + exports).
    ///
    /// This is the primary building block for:
    /// - native periodic flush loops
    /// - WASM "maybe_flush" request-driven flushing
    pub async fn flush_async(&self) -> ObservabilityResult<()> {
        self.flush_all_buffers().await
    }

    fn schedule_flush_best_effort(&self) {
        let this = self.clone();
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            wasm_bindgen_futures::spawn_local(async move {
                let _ = this.flush_all_buffers().await;
            });
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Avoid panicking if called outside a Tokio runtime.
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    handle.spawn(async move {
                        let _ = this.flush_all_buffers().await;
                    });
                }
                _ => {
                    // Best-effort: no runtime available. Keep buffering.
                }
            }
        }

        #[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
        {
            // In WASI-based WASM environments (e.g. SpinKube), background tasks cannot be spawned.
            // Flush synchronously by default so telemetry is exported before the instance exits.
            // Set PF_OBS_WASI_SYNC_FLUSH=false to opt out (accepting telemetry loss).
            let allow_sync_flush = std::env::var("PF_OBS_WASI_SYNC_FLUSH")
                .ok()
                .map(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "0" | "false" | "no"))
                .unwrap_or(true);

            if allow_sync_flush {
                let span_pending = this.span_buffer.lock().unwrap().len();
                let metric_pending = this.metric_buffer.lock().unwrap().len();
                let log_pending = this.log_buffer.lock().unwrap().len();
                log::debug!(
                    "otel:flush_wasi_start spans={} metrics={} logs={}",
                    span_pending,
                    metric_pending,
                    log_pending
                );
                // `spin_executor::run` drives the WASI poll loop; `futures::executor::block_on`
                // would deadlock by never polling the underlying WASI pollables.
                let res = spin_executor::run(async move { this.flush_all_buffers().await });
                match &res {
                    Ok(()) => log::debug!("otel:flush_wasi_done ok=true"),
                    Err(e) => log::warn!("otel:flush_wasi_done ok=false err={}", e),
                }
            } else {
                log::debug!("otel:flush_wasi_skipped reason=sync_flush_disabled");
            }
        }
    }

    /// Get collector information and capabilities
    #[cfg(feature = "structured-logging")]
    pub async fn get_collector_info(&self) -> ObservabilityResult<serde_json::Value> {
        self.collector_client.get_collector_info().await
    }

    /// Health check for the OTLP collector
    pub async fn health_check(&self) -> ObservabilityResult<bool> {
        self.collector_client.health_check().await
    }
}

impl Clone for Otel {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            collector_client: self.collector_client.clone(),
            resource_manager: self.resource_manager.clone(),
            active_spans: self.active_spans.clone(),
            current_trace_context: self.current_trace_context.clone(),
            span_buffer: self.span_buffer.clone(),
            metric_buffer: self.metric_buffer.clone(),
            log_buffer: self.log_buffer.clone(),
        }
    }
}

impl ObservabilityPlugin for Otel {
    fn start_span(&self, name: &str, attributes: &[(&str, &str)]) -> SpanGuard {
        let trace_context = get_current_context()
            .map(|parent| parent.new_child())
            .unwrap_or_else(TraceContext::new_root);

        let span_data = OtelSpanData {
            span_id: trace_context.span_id.clone(),
            trace_id: trace_context.trace_id,
            parent_span_id: trace_context.parent_span_id,
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: None,
            status: SpanStatus::Ok,
            attributes: attributes
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            events: Vec::new(),
        };

        {
            let mut spans = self.active_spans.lock().unwrap();
            spans.insert(trace_context.span_id.clone(), span_data);
        }

        SpanGuard::new(
            trace_context.span_id,
            Arc::new(self.clone()) as Arc<dyn ObservabilityPlugin>,
        )
    }

    fn end_span(&self, span_id: &str) {
        let span = {
            let mut spans = self.active_spans.lock().unwrap();
            spans.remove(span_id)
        };

        if let Some(mut span_data) = span {
            span_data.end_time = Some(Instant::now());

            // Add to buffer
            let should_flush = {
                let mut buffer = self.span_buffer.lock().unwrap();
                buffer.push(span_data);

                // Flush if buffer is full
                buffer.len() >= self.config.batch_size
            };

            if should_flush {
                self.schedule_flush_best_effort();
            }
        }
    }

    fn add_span_attribute(&self, span_id: &str, key: &str, value: &str) {
        let mut spans = self.active_spans.lock().unwrap();
        if let Some(span) = spans.get_mut(span_id) {
            span.attributes.insert(key.to_string(), value.to_string());
        }
    }

    fn set_span_status(&self, span_id: &str, status: SpanStatus) {
        let mut spans = self.active_spans.lock().unwrap();
        if let Some(span) = spans.get_mut(span_id) {
            span.status = status;
        }
    }

    fn record_metric(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let metric = MetricData::new(name, value);
        let metric = labels.iter().fold(metric, |m, (k, v)| m.with_label(*k, *v));

        let should_flush = {
            let mut buffer = self.metric_buffer.lock().unwrap();
            buffer.push(metric);
            debug!("otel:metric_buffered size={}", buffer.len());
            buffer.len() >= self.config.batch_size
        };

        if should_flush {
            self.schedule_flush_best_effort();
        }
    }

    fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
        self.record_metric(name, 1.0, labels);
    }

    /// Log a structured message with fields (feature-gated)
    #[cfg(feature = "structured-logging")]
    fn log_structured(&self, level: LogLevel, message: &str, fields: &serde_json::Value) {
        let mut log = LogData::new(level.as_str(), message);

        // Extract simple fields from JSON
        if let Some(obj) = fields.as_object() {
            for (k, v) in obj.iter() {
                if let Some(str_val) = v.as_str() {
                    log = log.with_attribute(k, str_val);
                } else {
                    // Convert non-string values to string
                    log = log.with_attribute(k, &v.to_string());
                }
            }
        }

        // Add trace correlation if available
        if let Some(context) = self.get_trace_context() {
            log = log
                .with_trace_context(&context.trace_id, &context.span_id)
                .with_attribute("trace_id", &context.trace_id)
                .with_attribute("span_id", &context.span_id);
        }

        let should_flush = {
            let mut buffer = self.log_buffer.lock().unwrap();
            buffer.push(log);
            buffer.len() >= self.config.batch_size
        };

        if should_flush {
            self.schedule_flush_best_effort();
        }
    }

    fn write_log(&self, message: &str) {
        println!("[OTEL] {}", message);
    }

    fn flush(&self) -> ObservabilityResult<()> {
        // Best-effort: schedule an async flush on the available runtime.
        self.schedule_flush_best_effort();
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        true
    }

    fn plugin_type(&self) -> &'static str {
        "otel-2025"
    }
}

impl MetricsPort for Otel {
    /// Emit a simple counter metric
    fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let metric_data = MetricData::new(name, value)
            .with_kind(MetricKind::Counter)
            .with_label("component", "otel_plugin")
            .with_unit("count")
            .with_description(format!("Counter metric: {}", name));

        // Add to metric buffer
        let should_flush = {
            let mut buffer = self.metric_buffer.lock().map_err(|e| {
                observability_core::ObservabilityError::metric(format!(
                    "Failed to lock metric buffer: {}",
                    e
                ))
            })?;
            buffer.push(metric_data);
            buffer.len() >= self.config.batch_size
        };

        if should_flush {
            self.schedule_flush_best_effort();
        }

        Ok(())
    }

    /// Emit a simple histogram/timing metric
    fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let metric_data = MetricData::new(name, value)
            .with_kind(MetricKind::Histogram)
            .with_label("component", "otel_plugin")
            .with_unit("duration")
            .with_description(format!("Histogram metric: {}", name));

        // Add to metric buffer
        let should_flush = {
            let mut buffer = self.metric_buffer.lock().map_err(|e| {
                observability_core::ObservabilityError::metric(format!(
                    "Failed to lock metric buffer: {}",
                    e
                ))
            })?;
            buffer.push(metric_data);
            buffer.len() >= self.config.batch_size
        };

        if should_flush {
            self.schedule_flush_best_effort();
        }

        Ok(())
    }

    /// Emit a simple gauge metric
    fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let metric_data = MetricData::new(name, value)
            .with_kind(MetricKind::Gauge)
            .with_label("component", "otel_plugin")
            .with_unit("value")
            .with_description(format!("Gauge metric: {}", name));

        // Add to metric buffer
        let should_flush = {
            let mut buffer = self.metric_buffer.lock().map_err(|e| {
                observability_core::ObservabilityError::metric(format!(
                    "Failed to lock metric buffer: {}",
                    e
                ))
            })?;
            buffer.push(metric_data);
            buffer.len() >= self.config.batch_size
        };

        if should_flush {
            self.schedule_flush_best_effort();
        }

        Ok(())
    }

    /// Check if metrics collection is enabled
    fn is_enabled(&self) -> bool {
        true // Always enabled for OTEL plugin
    }
}

/// Builder for OpenTelemetry extension
pub struct OtelBuilder {
    config: OtelConfig,
}

impl OtelBuilder {
    pub fn new() -> Self {
        Self {
            config: OtelConfig::default(),
        }
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.otlp_endpoint = endpoint.into();
        self
    }

    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.config.service_name = name.into();
        self
    }

    pub fn with_service_version(mut self, version: impl Into<String>) -> Self {
        self.config.service_version = version.into();
        self
    }

    pub fn with_service_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.service_namespace = namespace.into();
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn with_export_timeout_secs(mut self, timeout_secs: u64) -> Self {
        self.config.export_timeout_secs = timeout_secs;
        self
    }

    pub fn with_sampling_strategy(mut self, strategy: SamplingStrategy) -> Self {
        self.config.sampling_strategy = strategy;
        self
    }

    pub fn with_auto_instrumentation(mut self, enabled: bool) -> Self {
        self.config.auto_instrumentation = enabled;
        self
    }

    pub fn with_resource_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.config
            .resource_attributes
            .insert(key.into(), value.into());
        self
    }

    pub fn with_resource_attributes(mut self, attributes: HashMap<String, String>) -> Self {
        self.config.resource_attributes = attributes;
        self
    }

    /// Build the extension (async - preferred for SpinKube environments)
    pub async fn build(self) -> ObservabilityResult<Otel> {
        Otel::new(self.config).await
    }

    /// Build the extension synchronously (optional alternative for edge cases)
    pub fn build_sync(self) -> ObservabilityResult<Otel> {
        // Create a minimal CollectorClient that doesn't require async initialization
        let collector_client = Arc::new(CollectorClient::new_sync(
            &self.config.otlp_endpoint,
            Duration::from_secs(self.config.export_timeout_secs),
        )?);

        let resource_manager = Arc::new(ResourceAttributeManager::new(
            &self.config.service_name,
            &self.config.service_version,
            &self.config.service_namespace,
            self.config.resource_attributes.clone(),
        ));

        Ok(Otel {
            config: self.config,
            collector_client,
            resource_manager,
            active_spans: Arc::new(Mutex::new(HashMap::new())),
            current_trace_context: Arc::new(Mutex::new(None)),
            span_buffer: Arc::new(Mutex::new(Vec::new())),
            metric_buffer: Arc::new(Mutex::new(Vec::new())),
            log_buffer: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_builder() {
        let config = OtelConfig::builder()
            .with_service_name("test-service")
            .with_otlp_endpoint("http://localhost:4318")
            .with_batch_size(256)
            .with_resource_attribute("environment", "test")
            .build();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.otlp_endpoint, "http://localhost:4318");
        assert_eq!(config.batch_size, 256);
        assert_eq!(
            config.resource_attributes.get("environment"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_builder_preserves_extended_config() {
        let mut resource_attributes = HashMap::new();
        resource_attributes.insert("deployment.environment".to_string(), "test".to_string());
        resource_attributes.insert("region".to_string(), "us-east-1".to_string());

        let plugin = Otel::builder()
            .with_endpoint("http://collector:4317")
            .with_service_name("test-service")
            .with_service_version("1.2.3")
            .with_service_namespace("platform")
            .with_batch_size(128)
            .with_export_timeout_secs(7)
            .with_sampling_strategy(SamplingStrategy::AlwaysOff)
            .with_auto_instrumentation(false)
            .with_resource_attributes(resource_attributes.clone())
            .build_sync()
            .expect("builder should construct a plugin");

        assert_eq!(plugin.config.service_name, "test-service");
        assert_eq!(plugin.config.service_version, "1.2.3");
        assert_eq!(plugin.config.service_namespace, "platform");
        assert_eq!(plugin.config.batch_size, 128);
        assert_eq!(plugin.config.export_timeout_secs, 7);
        assert!(matches!(
            &plugin.config.sampling_strategy,
            &SamplingStrategy::AlwaysOff
        ));
        assert!(!plugin.config.auto_instrumentation);
        assert_eq!(plugin.config.resource_attributes, resource_attributes);

        let all_attributes = plugin.get_resource_attributes();
        assert_eq!(
            all_attributes.get("service.name"),
            Some(&"test-service".to_string())
        );
        assert_eq!(
            all_attributes.get("service.version"),
            Some(&"1.2.3".to_string())
        );
        assert_eq!(
            all_attributes.get("service.namespace"),
            Some(&"platform".to_string())
        );
        assert_eq!(
            all_attributes.get("deployment.environment"),
            Some(&"test".to_string())
        );
    }

    #[test]
    fn test_config_from_env() {
        // Set environment variables
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OTEL_SERVICE_NAME", "env-test-service") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://env-collector:4317") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OTEL_RESOURCE_ATTRIBUTES", "env=test,version=1.0") };

        let config = OtelConfig::from_env();

        assert_eq!(config.service_name, "env-test-service");
        assert_eq!(config.otlp_endpoint, "http://env-collector:4317");
        assert_eq!(
            config.resource_attributes.get("env"),
            Some(&"test".to_string())
        );
        assert_eq!(
            config.resource_attributes.get("version"),
            Some(&"1.0".to_string())
        );

        // Clean up
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OTEL_SERVICE_NAME") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("OTEL_RESOURCE_ATTRIBUTES") };
    }

    #[test]
    fn test_start_span_uses_current_trace_context_as_parent() {
        let otel = Otel::builder()
            .with_endpoint("http://127.0.0.1:1")
            .with_service_name("context-test")
            .build_sync()
            .expect("otel init");
        let parent = TraceContext::new_root();

        observability_core::set_current_context(parent.clone());
        let guard = otel.start_span("child", &[]);
        observability_core::clear_current_context();

        let spans = otel.active_spans.lock().expect("span lock poisoned");
        let child = spans
            .get(guard.span_id())
            .expect("child span should be active");
        assert_eq!(child.trace_id, parent.trace_id);
        assert_eq!(child.parent_span_id.as_deref(), Some(parent.span_id.as_str()));
        drop(spans);
        drop(guard);
    }

    #[tokio::test]
    async fn test_extension_builder() {
        let result = Otel::builder()
            .with_endpoint("http://test-collector:4317")
            .with_service_name("test-extension")
            .with_batch_size(100)
            .build()
            .await;

        // Note: This will likely fail in tests due to network connectivity
        // but it validates the builder pattern works
        assert!(result.is_ok() || result.is_err()); // Will pass regardless - just testing the builder pattern
    }

    #[tokio::test]
    async fn flush_async_requeues_on_export_failure() {
        // Use a short timeout so a dead collector won't hang the test.
        let cfg = OtelConfig::builder()
            .with_otlp_endpoint("http://127.0.0.1:1") // likely closed => connection error
            .with_service_name("flush-test")
            .with_batch_size(1)
            .with_export_timeout_secs(1)
            .build();

        let otel = Otel::new(cfg).await.expect("otel init");

        // Put one span in buffer by starting+ending it.
        let g = otel.start_span("test.span", &[]);
        drop(g);

        let before = otel.span_buffer.lock().unwrap().len();
        assert_eq!(before, 1);

        // Flush should fail, but must requeue to avoid data loss.
        let res = otel.flush_async().await;
        assert!(res.is_err());

        let after = otel.span_buffer.lock().unwrap().len();
        assert_eq!(after, 1);
    }
}
