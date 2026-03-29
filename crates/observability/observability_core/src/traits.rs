//! Core traits for the observability plugin system

use crate::error::ObservabilityResult;
use std::collections::HashMap;
use std::sync::Arc;
use web_time::{Duration, Instant};

#[cfg(feature = "structured-logging")]
use serde_json::Value as JsonValue;

/// Log levels for structured logging
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "structured-logging",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum LogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN",
            LogLevel::Info => "INFO",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        }
    }
}

/// Span guard that automatically ends spans when dropped
pub struct SpanGuard {
    span_id: String,
    start_time: Instant,
    plugin: Option<Arc<dyn ObservabilityPlugin>>,
}

impl SpanGuard {
    pub fn new(span_id: String, plugin: Arc<dyn ObservabilityPlugin>) -> Self {
        Self {
            span_id,
            start_time: Instant::now(),
            plugin: Some(plugin),
        }
    }

    pub fn no_op() -> Self {
        Self {
            span_id: String::new(),
            start_time: Instant::now(),
            plugin: None,
        }
    }

    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn add_attribute(&self, key: &str, value: &str) {
        if let Some(plugin) = &self.plugin {
            plugin.add_span_attribute(&self.span_id, key, value);
        }
    }

    pub fn set_status(&self, status: SpanStatus) {
        if let Some(plugin) = &self.plugin {
            plugin.set_span_status(&self.span_id, status);
        }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        if let Some(plugin) = self.plugin.take() {
            plugin.end_span(&self.span_id);
        }
    }
}

/// Status of a span
#[derive(Debug, Clone, Copy)]
pub enum SpanStatus {
    Ok,
    Error,
    Cancelled,
}

/// Core observability plugin trait
pub trait ObservabilityPlugin: Send + Sync {
    /// Start a new span and return a guard
    fn start_span(&self, name: &str, attributes: &[(&str, &str)]) -> SpanGuard;

    /// End a span by ID
    fn end_span(&self, span_id: &str);

    /// Add an attribute to an existing span
    fn add_span_attribute(&self, span_id: &str, key: &str, value: &str);

    /// Set the status of a span
    fn set_span_status(&self, span_id: &str, status: SpanStatus);

    /// Record a metric with labels
    fn record_metric(&self, name: &str, value: f64, labels: &[(&str, &str)]);

    /// Increment a counter metric
    fn increment_counter(&self, name: &str, labels: &[(&str, &str)]) {
        self.record_metric(name, 1.0, labels);
    }

    /// Record a histogram value
    fn record_histogram(&self, name: &str, value: f64, labels: &[(&str, &str)]) {
        self.record_metric(name, value, labels);
    }

    /// Log a structured message with fields
    #[cfg(feature = "structured-logging")]
    fn log_structured(&self, level: LogLevel, message: &str, fields: &JsonValue);

    /// Log a simple message
    fn log(&self, level: LogLevel, message: &str) {
        #[cfg(feature = "structured-logging")]
        self.log_structured(level, message, &serde_json::json!({}));

        #[cfg(not(feature = "structured-logging"))]
        {
            // Simple console output for WASM
            let level_str = level.as_str();
            let output = format!("[{}] {}", level_str, message);
            self.write_log(&output);
        }
    }

    /// Write log output (implementation-specific)
    fn write_log(&self, message: &str);

    /// Flush any pending telemetry data
    fn flush(&self) -> ObservabilityResult<()>;

    /// Check if the plugin is enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Get the plugin name/type
    fn plugin_type(&self) -> &'static str {
        "generic"
    }
}

/// Trait for collecting and managing metrics
pub trait MetricsCollector: Send + Sync {
    /// Register a new counter
    fn register_counter(
        &mut self,
        name: &str,
        description: &str,
        labels: &[&str],
    ) -> ObservabilityResult<()>;

    /// Register a new histogram  
    fn register_histogram(
        &mut self,
        name: &str,
        description: &str,
        buckets: &[f64],
        labels: &[&str],
    ) -> ObservabilityResult<()>;

    /// Register a new gauge
    fn register_gauge(
        &mut self,
        name: &str,
        description: &str,
        labels: &[&str],
    ) -> ObservabilityResult<()>;

    /// Record a counter increment
    fn record_counter(
        &self,
        name: &str,
        value: f64,
        labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()>;

    /// Record a histogram observation
    fn record_histogram(
        &self,
        name: &str,
        value: f64,
        labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()>;

    /// Set a gauge value
    fn set_gauge(
        &self,
        name: &str,
        value: f64,
        labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()>;

    /// Get current metric values (for testing/debugging)
    fn get_metrics(&self) -> HashMap<String, f64>;

    /// Clear all metrics
    fn clear(&mut self);
}

/// Trait for structured logging
#[cfg(feature = "structured-logging")]
pub trait StructuredLogger: Send + Sync {
    /// Log with trace context correlation
    fn log_with_trace(
        &self,
        level: LogLevel,
        message: &str,
        fields: &JsonValue,
        trace_id: Option<&str>,
        span_id: Option<&str>,
    );

    /// Log performance metrics
    fn log_performance(
        &self,
        operation: &str,
        duration: Duration,
        success: bool,
        additional_fields: &JsonValue,
    );

    /// Log errors with context
    fn log_error(&self, error: &dyn std::error::Error, context: &JsonValue);

    /// Set the minimum log level
    fn set_level(&mut self, level: LogLevel);

    /// Check if a level is enabled
    fn is_level_enabled(&self, level: LogLevel) -> bool;
}

/// Builder trait for creating observability plugins
pub trait ObservabilityBuilder {
    type Plugin: ObservabilityPlugin;

    /// Build the plugin with the current configuration
    fn build(self) -> ObservabilityResult<Self::Plugin>;

    /// Set the plugin name
    fn with_name(self, name: impl Into<String>) -> Self;

    /// Enable/disable the plugin
    fn enabled(self, enabled: bool) -> Self;
}

/// Trait for plugins that support batching
pub trait BatchingSupport {
    /// Get the current batch size
    fn batch_size(&self) -> usize;

    /// Set the batch size
    fn set_batch_size(&mut self, size: usize);

    /// Get the flush interval
    fn flush_interval(&self) -> Duration;

    /// Set the flush interval
    fn set_flush_interval(&mut self, interval: Duration);

    /// Force flush all batched data
    fn force_flush(&self) -> ObservabilityResult<()>;
}

/// Metric label allowlist for cardinality reduction.
///
/// Backends should treat unknown labels as optional or drop them to avoid cardinality explosions.
pub const METRIC_LABEL_ALLOWLIST: &[&str] = &[
    // Service identity (bounded)
    "app",       // Application name
    "version",   // Application version
    "namespace", // Kubernetes namespace
    // A2A / SDK (bounded)
    "component", // a2a_server | a2a_client | llm_client | sdk
    "operation", // rpc method / operation name (bounded set)
    "status",    // ok | error
    // LLM (bounded)
    "provider",  // LLM provider
    "model",     // LLM model name
    "direction", // input | output
];

/// Create a label map from key-value pairs
pub fn create_labels(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Validate that label keys are within the allowlist.
pub fn validate_metric_label_allowlist(label_keys: &[&str]) -> bool {
    label_keys
        .iter()
        .all(|label| METRIC_LABEL_ALLOWLIST.contains(label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct MockPlugin {
        end_calls: AtomicUsize,
        attrs: Mutex<Vec<(String, String, String)>>,
        statuses: Mutex<Vec<(String, SpanStatus)>>,
    }

    impl ObservabilityPlugin for MockPlugin {
        fn start_span(&self, _name: &str, _attributes: &[(&str, &str)]) -> SpanGuard {
            // Not used by these tests; guard behavior is tested directly.
            SpanGuard::no_op()
        }

        fn end_span(&self, span_id: &str) {
            self.end_calls.fetch_add(1, Ordering::SeqCst);
            // Ensure we receive the expected ID (helps catch regressions).
            assert!(!span_id.is_empty());
        }

        fn add_span_attribute(&self, span_id: &str, key: &str, value: &str) {
            self.attrs.lock().unwrap().push((
                span_id.to_string(),
                key.to_string(),
                value.to_string(),
            ));
        }

        fn set_span_status(&self, span_id: &str, status: SpanStatus) {
            self.statuses
                .lock()
                .unwrap()
                .push((span_id.to_string(), status));
        }

        fn record_metric(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {}

        #[cfg(feature = "structured-logging")]
        fn log_structured(&self, _level: LogLevel, _message: &str, _fields: &JsonValue) {}

        fn write_log(&self, _message: &str) {}

        fn flush(&self) -> ObservabilityResult<()> {
            Ok(())
        }
    }

    #[test]
    fn span_guard_drop_calls_end_span_once() {
        let typed = Arc::new(MockPlugin::default());
        let plugin: Arc<dyn ObservabilityPlugin> = typed.clone();

        {
            let g = SpanGuard::new("span-1".to_string(), plugin);
            g.add_attribute("k", "v");
            g.set_status(SpanStatus::Error);
        } // drop => end_span

        assert_eq!(typed.end_calls.load(Ordering::SeqCst), 1);

        let attrs = typed.attrs.lock().unwrap().clone();
        assert_eq!(
            attrs,
            vec![("span-1".to_string(), "k".to_string(), "v".to_string())]
        );

        let statuses = typed.statuses.lock().unwrap().clone();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].0, "span-1");
        assert!(matches!(statuses[0].1, SpanStatus::Error));
    }

    #[test]
    fn span_guard_no_op_is_safe() {
        // Should not panic, and should not call into any plugin.
        let g = SpanGuard::no_op();
        g.add_attribute("k", "v");
        g.set_status(SpanStatus::Error);
        drop(g);
    }
}
