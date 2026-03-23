//! Zero-cost no-op implementations for observability when features are disabled

use crate::error::ObservabilityResult;
#[cfg(feature = "structured-logging")]
use crate::traits::StructuredLogger;
use crate::traits::{LogLevel, MetricsCollector, ObservabilityPlugin, SpanGuard, SpanStatus};
use std::collections::HashMap;
use std::sync::Arc;
use web_time::Duration;

#[cfg(feature = "structured-logging")]
use serde_json::Value as JsonValue;

/// No-op observability plugin that does nothing
///
/// This implementation provides zero runtime cost when observability is disabled.
/// All methods are inlined and compile to nothing in release builds.
pub struct NoOpObservabilityPlugin;

impl NoOpObservabilityPlugin {
    /// Create a new no-op plugin
    #[inline]
    pub fn new() -> Self {
        Self
    }

    /// Create an Arc'd no-op plugin for sharing
    #[inline]
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for NoOpObservabilityPlugin {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl ObservabilityPlugin for NoOpObservabilityPlugin {
    #[inline]
    fn start_span(&self, _name: &str, _attributes: &[(&str, &str)]) -> SpanGuard {
        SpanGuard::no_op()
    }

    #[inline]
    fn end_span(&self, _span_id: &str) {
        // No-op
    }

    #[inline]
    fn add_span_attribute(&self, _span_id: &str, _key: &str, _value: &str) {
        // No-op
    }

    #[inline]
    fn set_span_status(&self, _span_id: &str, _status: SpanStatus) {
        // No-op
    }

    #[inline]
    fn record_metric(&self, _name: &str, _value: f64, _labels: &[(&str, &str)]) {
        // No-op
    }

    #[cfg(feature = "structured-logging")]
    #[inline]
    fn log_structured(&self, _level: LogLevel, _message: &str, _fields: &JsonValue) {
        // No-op
    }

    #[inline]
    fn write_log(&self, _message: &str) {
        // No-op - could optionally write to stdout in debug builds
        #[cfg(debug_assertions)]
        {
            // Uncomment for debug logging in development
            // println!("{}", _message);
        }
    }

    #[inline]
    fn flush(&self) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn is_enabled(&self) -> bool {
        false
    }

    #[inline]
    fn plugin_type(&self) -> &'static str {
        "noop"
    }
}

/// No-op metrics collector
pub struct NoOpMetricsCollector;

impl NoOpMetricsCollector {
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoOpMetricsCollector {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector for NoOpMetricsCollector {
    #[inline]
    fn register_counter(
        &mut self,
        _name: &str,
        _description: &str,
        _labels: &[&str],
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn register_histogram(
        &mut self,
        _name: &str,
        _description: &str,
        _buckets: &[f64],
        _labels: &[&str],
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn register_gauge(
        &mut self,
        _name: &str,
        _description: &str,
        _labels: &[&str],
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn record_counter(
        &self,
        _name: &str,
        _value: f64,
        _labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn record_histogram(
        &self,
        _name: &str,
        _value: f64,
        _labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn set_gauge(
        &self,
        _name: &str,
        _value: f64,
        _labels: &HashMap<String, String>,
    ) -> ObservabilityResult<()> {
        Ok(())
    }

    #[inline]
    fn get_metrics(&self) -> HashMap<String, f64> {
        HashMap::new()
    }

    #[inline]
    fn clear(&mut self) {
        // No-op
    }
}

/// No-op structured logger
#[cfg(feature = "structured-logging")]
pub struct NoOpStructuredLogger {
    level: LogLevel,
}

#[cfg(feature = "structured-logging")]
impl NoOpStructuredLogger {
    #[inline]
    pub fn new() -> Self {
        Self {
            level: LogLevel::Info,
        }
    }

    #[inline]
    pub fn with_level(level: LogLevel) -> Self {
        Self { level }
    }
}

#[cfg(feature = "structured-logging")]
impl Default for NoOpStructuredLogger {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "structured-logging")]
impl StructuredLogger for NoOpStructuredLogger {
    #[inline]
    fn log_with_trace(
        &self,
        _level: LogLevel,
        _message: &str,
        _fields: &JsonValue,
        _trace_id: Option<&str>,
        _span_id: Option<&str>,
    ) {
        // No-op
    }

    #[inline]
    fn log_performance(
        &self,
        _operation: &str,
        _duration: Duration,
        _success: bool,
        _additional_fields: &JsonValue,
    ) {
        // No-op
    }

    #[inline]
    fn log_error(&self, _error: &dyn std::error::Error, _context: &JsonValue) {
        // No-op
    }

    #[inline]
    fn set_level(&mut self, level: LogLevel) {
        self.level = level;
    }

    #[inline]
    fn is_level_enabled(&self, level: LogLevel) -> bool {
        level <= self.level
    }
}

/// Convenience function to create a no-op plugin when observability is disabled
#[inline]
pub fn create_noop_plugin() -> Box<dyn ObservabilityPlugin> {
    Box::new(NoOpObservabilityPlugin::new())
}

/// Convenience function to create a shared no-op plugin
#[inline]
pub fn create_shared_noop_plugin() -> Arc<dyn ObservabilityPlugin> {
    Arc::new(NoOpObservabilityPlugin::new())
}

/// Macro to conditionally create observability plugin based on features
#[macro_export]
macro_rules! observability_plugin {
    ($plugin_expr:expr_2021) => {
        #[cfg(feature = "observability")]
        {
            $plugin_expr
        }
        #[cfg(not(feature = "observability"))]
        {
            $crate::noop::create_noop_plugin()
        }
    };
}

/// Macro to conditionally create shared observability plugin based on features
#[macro_export]
macro_rules! shared_observability_plugin {
    ($plugin_expr:expr_2021) => {
        #[cfg(feature = "observability")]
        {
            $plugin_expr
        }
        #[cfg(not(feature = "observability"))]
        {
            $crate::noop::create_shared_noop_plugin()
        }
    };
}

/// Conditional span creation macro
#[macro_export]
macro_rules! observability_span {
    ($plugin:expr_2021, $name:expr_2021, $($attr_key:expr_2021 => $attr_val:expr_2021),*) => {
        {
            #[cfg(feature = "observability")]
            {
                $plugin.start_span($name, &[$(($attr_key, $attr_val)),*])
            }
            #[cfg(not(feature = "observability"))]
            {
                $crate::SpanGuard::no_op()
            }
        }
    };
    ($plugin:expr_2021, $name:expr_2021) => {
        {
            #[cfg(feature = "observability")]
            {
                $plugin.start_span($name, &[])
            }
            #[cfg(not(feature = "observability"))]
            {
                $crate::SpanGuard::no_op()
            }
        }
    };
}

/// Conditional metric recording macro
#[macro_export]
macro_rules! observability_metric {
    ($plugin:expr_2021, $name:expr_2021, $value:expr_2021, $($label_key:expr_2021 => $label_val:expr_2021),*) => {
        #[cfg(feature = "observability")]
        {
            $plugin.record_metric($name, $value, &[$(($label_key, $label_val)),*]);
        }
        #[cfg(not(feature = "observability"))]
        {
            // Compile to nothing
        }
    };
    ($plugin:expr_2021, $name:expr_2021, $value:expr_2021) => {
        #[cfg(feature = "observability")]
        {
            $plugin.record_metric($name, $value, &[]);
        }
        #[cfg(not(feature = "observability"))]
        {
            // Compile to nothing
        }
    };
}

/// Conditional logging macro
#[macro_export]
macro_rules! observability_log {
    ($plugin:expr_2021, $level:expr_2021, $message:expr_2021) => {
        #[cfg(feature = "observability")]
        {
            $plugin.log($level, $message);
        }
        #[cfg(not(feature = "observability"))]
        {
            // Compile to nothing in release, optionally log in debug
            #[cfg(debug_assertions)]
            {
                println!("[{}] {}", $level.as_str(), $message);
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_plugin_zero_cost() {
        let plugin = NoOpObservabilityPlugin::new();

        // These should all compile to no-ops
        let _span = plugin.start_span("test", &[("key", "value")]);
        plugin.record_metric("test_metric", 1.0, &[("label", "value")]);
        plugin.log(LogLevel::Info, "test message");

        assert!(!plugin.is_enabled());
        assert_eq!(plugin.plugin_type(), "noop");
        assert!(plugin.flush().is_ok());
    }

    #[test]
    fn test_noop_metrics_collector() {
        let mut collector = NoOpMetricsCollector::new();

        // All operations should succeed but do nothing
        assert!(collector
            .register_counter("test", "description", &["label"])
            .is_ok());
        assert!(collector
            .record_counter("test", 1.0, &HashMap::new())
            .is_ok());
        assert!(collector.get_metrics().is_empty());

        collector.clear(); // Should not panic
    }

    #[cfg(feature = "structured-logging")]
    #[test]
    fn test_noop_structured_logger() {
        let mut logger = NoOpStructuredLogger::new();

        // All operations should succeed but do nothing
        use serde_json::json;
        logger.log_with_trace(LogLevel::Info, "test", &json!({}), None, None);
        logger.log_performance("test_op", Duration::from_millis(100), true, &json!({}));

        logger.set_level(LogLevel::Debug);
        assert!(logger.is_level_enabled(LogLevel::Info));
        assert!(!logger.is_level_enabled(LogLevel::Trace));
    }

    #[test]
    fn test_macros_compile() {
        let _plugin = create_noop_plugin();

        // Test that macros compile without errors
        let _span = observability_span!(_plugin, "test_span", "key" => "value");
        observability_metric!(_plugin, "test_metric", 1.0, "label" => "value");
        observability_log!(_plugin, LogLevel::Info, "test message");
    }
}
