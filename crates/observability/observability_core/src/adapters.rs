//! Adapters layer - Concrete implementations of ports for external integrations
//!
//! This module contains WASM-compatible implementations that connect our core domain
//! to external systems like stdout, HTTP endpoints, and standard logging frameworks.

use crate::domain::{LogEntry, ProcessorChain};
use crate::error::{ObservabilityError, ObservabilityResult};
use crate::ports::{FormatterPort, StandardLoggingPort, TransportPort};
use crate::traits::LogLevel;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ==================== FORMATTERS ====================

/// JSON formatter for structured logging output
pub struct JsonFormatter;

impl FormatterPort for JsonFormatter {
    fn format(&self, entry: &LogEntry) -> ObservabilityResult<String> {
        serde_json::to_string(entry)
            .map_err(|e| ObservabilityError::logging(format!("JSON formatting failed: {}", e)))
    }
}

/// Compact JSON formatter (single line)
pub struct CompactJsonFormatter;

impl FormatterPort for CompactJsonFormatter {
    fn format(&self, entry: &LogEntry) -> ObservabilityResult<String> {
        let mut output = String::new();

        // Add timestamp
        output.push_str(&format!(
            "{} ",
            entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f")
        ));

        // Add level
        output.push_str(&format!("[{}] ", entry.level.as_str().to_uppercase()));

        // Add message
        output.push_str(&entry.message);

        // Add structured fields if any
        if let serde_json::Value::Object(ref fields) = entry.fields {
            if !fields.is_empty() {
                output.push(' ');
                output.push_str(&serde_json::to_string(fields).map_err(|e| {
                    ObservabilityError::logging(format!("Field serialization failed: {}", e))
                })?);
            }
        }

        // Add trace context if available
        if let Some(ref trace) = entry.trace_context {
            output.push_str(&format!(
                " trace_id={} span_id={}",
                trace.trace_id, trace.span_id
            ));
        }

        Ok(output)
    }
}

/// Plain text formatter for human-readable output
pub struct PlainTextFormatter;

impl FormatterPort for PlainTextFormatter {
    fn format(&self, entry: &LogEntry) -> ObservabilityResult<String> {
        let mut output = String::new();

        // Timestamp
        output.push_str(&format!(
            "{} ",
            entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f")
        ));

        // Level with colors/styling (WASM-compatible)
        let level_str = match entry.level {
            LogLevel::Error => "ERROR",
            LogLevel::Warn => "WARN ",
            LogLevel::Info => "INFO ",
            LogLevel::Debug => "DEBUG",
            LogLevel::Trace => "TRACE",
        };
        output.push_str(&format!("[{}] ", level_str));

        // Module path if available
        if let Some(ref module) = entry.source.module {
            output.push_str(&format!("{}: ", module));
        }

        // Message
        output.push_str(&entry.message);

        Ok(output)
    }
}

// ==================== TRANSPORTS ====================

/// WASM stdout transport adapter
pub struct WasmStdoutAdapter {
    formatter: Box<dyn FormatterPort>,
}

impl WasmStdoutAdapter {
    pub fn new(formatter: Box<dyn FormatterPort>) -> Self {
        Self { formatter }
    }

    pub fn with_json_formatter() -> Self {
        Self::new(Box::new(JsonFormatter))
    }

    pub fn with_compact_formatter() -> Self {
        Self::new(Box::new(CompactJsonFormatter))
    }

    pub fn with_plain_text_formatter() -> Self {
        Self::new(Box::new(PlainTextFormatter))
    }
}

impl TransportPort for WasmStdoutAdapter {
    fn transport(&self, entry: &LogEntry) -> ObservabilityResult<()> {
        let formatted = self.formatter.format(entry)?;

        // WASM-compatible stdout output
        println!("{}", formatted);

        Ok(())
    }
}

/// No-op transport for testing/disabled logging
pub struct NoOpTransport;

impl TransportPort for NoOpTransport {
    fn transport(&self, _entry: &LogEntry) -> ObservabilityResult<()> {
        // Do nothing
        Ok(())
    }
}

// ==================== LOG DIRECTIVES ====================

/// Parsed `RUST_LOG`-style directives for per-target log filtering.
///
/// Supports: `"info"`, `"info,agent_sdk=debug,a2a_protocol_core=trace"`.
/// Unknown tokens are silently ignored.
#[derive(Debug, Clone)]
pub struct LogDirectives {
    global: LogLevel,
    targets: Vec<(String, LogLevel)>,
    max_level: LogLevel,
}

impl LogDirectives {
    pub fn from_level(level: LogLevel) -> Self {
        Self {
            global: level,
            targets: Vec::new(),
            max_level: level,
        }
    }

    /// Parse a `RUST_LOG`-style directive string.
    ///
    /// Examples: `"info"`, `"info,agent_sdk=debug"`, `"warn,a2a_protocol_core=trace,llm_client=debug"`.
    pub fn parse(s: &str) -> Self {
        let mut global = LogLevel::Info;
        let mut targets = Vec::new();
        let mut max = LogLevel::Info;

        for part in s.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((target, level_str)) = part.split_once('=') {
                if let Some(lvl) = Self::str_to_level(level_str.trim()) {
                    targets.push((target.trim().to_string(), lvl));
                    if lvl > max {
                        max = lvl;
                    }
                }
            } else if let Some(lvl) = Self::str_to_level(part) {
                global = lvl;
                if lvl > max {
                    max = lvl;
                }
            }
        }
        if global > max {
            max = global;
        }
        Self {
            global,
            targets,
            max_level: max,
        }
    }

    pub(crate) fn str_to_level(s: &str) -> Option<LogLevel> {
        match s.to_ascii_lowercase().as_str() {
            "error" => Some(LogLevel::Error),
            "warn" => Some(LogLevel::Warn),
            "info" => Some(LogLevel::Info),
            "debug" => Some(LogLevel::Debug),
            "trace" => Some(LogLevel::Trace),
            _ => None,
        }
    }

    pub fn global_level(&self) -> LogLevel {
        self.global
    }

    pub fn max_level(&self) -> LogLevel {
        self.max_level
    }

    /// Check whether a log record at `level` from `target` should be emitted.
    pub fn enabled(&self, level: LogLevel, target: &str) -> bool {
        for (prefix, directive_level) in &self.targets {
            if target.starts_with(prefix.as_str()) {
                return level <= *directive_level;
            }
        }
        level <= self.global
    }
}

// ==================== STANDARD LOGGING INTEGRATION ====================

/// Standard logging adapter that hooks into Rust's log crate
pub struct StandardLogAdapter {
    processor_chain: ProcessorChain,
    transport: Arc<dyn TransportPort>,
    directives: LogDirectives,
}

impl StandardLogAdapter {
    pub fn new(
        processor_chain: ProcessorChain,
        transport: Arc<dyn TransportPort>,
        directives: LogDirectives,
    ) -> Self {
        Self {
            processor_chain,
            transport,
            directives,
        }
    }

    /// Convert log::Record to our LogEntry
    fn record_to_log_entry(&self, record: &log::Record) -> LogEntry {
        use crate::domain::LogSource;
        use crate::domain::TraceCorrelation;

        // Extract structured fields from log::kv
        let kv_fields = crate::domain::LogKvExtractor::extract_kv_from_record(record);

        // Best-effort trace correlation:
        // If the SDK (A2A server/client) has set a thread-local trace context,
        // attach it to the log entry so downstream processors can emit trace_id/span_id.
        let trace_context = crate::context::get_current_context().map(|ctx| TraceCorrelation {
            trace_id: ctx.trace_id,
            span_id: ctx.span_id,
            parent_span_id: ctx.parent_span_id,
        });

        LogEntry {
            timestamp: chrono::Utc::now(),
            level: self.convert_log_level(record.level()),
            message: record.args().to_string(),
            fields: kv_fields, // Use extracted kv fields instead of empty object
            trace_context,
            source: LogSource {
                module: record.module_path().map(|s| s.to_string()),
                file: record.file().map(|s| s.to_string()),
                line: record.line(),
                target: Some(record.target().to_string()),
            },
        }
    }

    /// Convert log::Level to our LogLevel
    fn convert_log_level(&self, level: log::Level) -> LogLevel {
        match level {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Trace,
        }
    }

    /// Convert our LogLevel to log::Level
    fn convert_to_log_level(&self, level: &LogLevel) -> log::Level {
        match level {
            LogLevel::Error => log::Level::Error,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Info => log::Level::Info,
            LogLevel::Debug => log::Level::Debug,
            LogLevel::Trace => log::Level::Trace,
        }
    }
}

impl StandardLoggingPort for StandardLogAdapter {
    fn initialize(&self) -> ObservabilityResult<()> {
        log::set_max_level(
            self.convert_to_log_level(&self.directives.max_level())
                .to_level_filter(),
        );

        Ok(())
    }

    fn process_standard_log(&self, entry: LogEntry) -> ObservabilityResult<()> {
        let processed_entry = self.processor_chain.process(entry)?;
        self.transport.transport(&processed_entry)?;
        Ok(())
    }

    fn enabled(&self, level: &LogLevel) -> bool {
        *level <= self.directives.global_level()
    }
}

impl log::Log for StandardLogAdapter {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let level = self.convert_log_level(metadata.level());
        self.directives.enabled(level, metadata.target())
    }

    fn log(&self, record: &log::Record) {
        let level = self.convert_log_level(record.level());
        if self.directives.enabled(level, record.target()) {
            let log_entry = self.record_to_log_entry(record);
            if let Err(e) = self.process_standard_log(log_entry) {
                eprintln!("Logging error: {}", e);
            }
        }
    }

    fn flush(&self) {
        // WASM stdout is immediately flushed
    }
}

// ==================== CONTEXT ADAPTER ====================

/// Thread-local context adapter (WASM-compatible)
pub struct WasmContextAdapter {
    // Using Arc<Mutex<>> since WASM doesn't have real threads
    context: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl WasmContextAdapter {
    pub fn new() -> Self {
        Self {
            context: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl crate::ports::ContextPort for WasmContextAdapter {
    fn get_context(&self) -> HashMap<String, serde_json::Value> {
        self.context.lock().unwrap().clone()
    }

    fn add_context(&self, key: String, value: serde_json::Value) {
        self.context.lock().unwrap().insert(key, value);
    }

    fn remove_context(&self, key: &str) {
        self.context.lock().unwrap().remove(key);
    }

    fn clear_context(&self) {
        self.context.lock().unwrap().clear();
    }
}

impl Default for WasmContextAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== BUILDER UTILITIES ====================

/// Builder for creating a complete logging setup
pub struct LoggingSetupBuilder {
    processor_chain: Option<ProcessorChain>,
    transport: Option<Arc<dyn TransportPort>>,
    directives: LogDirectives,
}

impl LoggingSetupBuilder {
    pub fn new() -> Self {
        Self {
            processor_chain: None,
            transport: None,
            directives: LogDirectives::from_level(LogLevel::Info),
        }
    }

    pub fn with_processor_chain(mut self, chain: ProcessorChain) -> Self {
        self.processor_chain = Some(chain);
        self
    }

    pub fn with_transport(mut self, transport: Arc<dyn TransportPort>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_level_filter(mut self, level: LogLevel) -> Self {
        self.directives = LogDirectives::from_level(level);
        self
    }

    pub fn with_directives(mut self, directives: LogDirectives) -> Self {
        self.directives = directives;
        self
    }

    pub fn build(self) -> ObservabilityResult<StandardLogAdapter> {
        let processor_chain = self
            .processor_chain
            .unwrap_or_else(crate::domain::build_default_processor_chain);

        let transport = self
            .transport
            .unwrap_or_else(|| Arc::new(WasmStdoutAdapter::with_compact_formatter()));

        Ok(StandardLogAdapter::new(
            processor_chain,
            transport,
            self.directives,
        ))
    }
}

impl Default for LoggingSetupBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== TRACING INTEGRATION ====================

/// Tracing subscriber adapter for structured logging integration
///
/// This adapter implements tracing::Subscriber to capture tracing events
/// and convert them to LogEntry for processing through the hexagonal architecture
#[cfg(feature = "structured-logging")]
pub struct TracingSubscriberAdapter {
    processor_chain: ProcessorChain,
    transport: Arc<dyn TransportPort>,
    level_filter: LogLevel,
}

#[cfg(feature = "structured-logging")]
impl TracingSubscriberAdapter {
    pub fn new(
        processor_chain: ProcessorChain,
        transport: Arc<dyn TransportPort>,
        level_filter: LogLevel,
    ) -> Self {
        Self {
            processor_chain,
            transport,
            level_filter,
        }
    }

    /// Convert tracing event to LogEntry
    fn event_to_log_entry(&self, event: &tracing::Event<'_>) -> LogEntry {
        use crate::domain::LogSource;

        // Extract event metadata
        let metadata = event.metadata();
        let level = self.convert_tracing_level(*metadata.level());

        // Create visitor to extract fields and message
        let mut visitor = TracingFieldVisitor::new();
        event.record(&mut visitor);

        // Get current span context if available
        let span_context = self.get_current_span_context();

        LogEntry {
            timestamp: chrono::Utc::now(),
            level,
            message: visitor
                .message
                .unwrap_or_else(|| "tracing event".to_string()),
            fields: serde_json::Value::Object(visitor.fields),
            trace_context: span_context,
            source: LogSource {
                module: metadata.module_path().map(|s| s.to_string()),
                file: metadata.file().map(|s| s.to_string()),
                line: metadata.line(),
                target: Some(metadata.target().to_string()),
            },
        }
    }

    /// Convert tracing::Level to our LogLevel
    fn convert_tracing_level(&self, level: tracing::Level) -> LogLevel {
        match level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }

    /// Get current span context from tracing
    fn get_current_span_context(&self) -> Option<crate::domain::TraceCorrelation> {
        // Use tracing's span system to get current context
        let current_span = tracing::Span::current();
        if current_span.is_none() {
            return None;
        }

        // Extract span ID and trace ID from current span
        // This is a simplified implementation - in production you'd want
        // to integrate with proper W3C trace context propagation
        let span_id = format!("{:016x}", current_span.id()?.into_u64());

        // For now, generate a trace ID from span hierarchy
        // In a full implementation, this would come from W3C headers
        let trace_id = self.generate_trace_id_from_span(&current_span);

        Some(crate::domain::TraceCorrelation {
            trace_id,
            span_id,
            parent_span_id: None, // TODO: Extract parent span ID
        })
    }

    /// Generate trace ID from span (simplified implementation)
    fn generate_trace_id_from_span(&self, span: &tracing::Span) -> String {
        // This is a placeholder - in production you'd extract from W3C headers
        format!(
            "trace-{:032x}",
            span.id().map(|id| id.into_u64()).unwrap_or(0)
        )
    }
}

#[cfg(feature = "structured-logging")]
impl tracing::Subscriber for TracingSubscriberAdapter {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        let level = self.convert_tracing_level(*metadata.level());
        level <= self.level_filter
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        // Create a new span ID
        // This is a simplified implementation
        let id = rand::random::<u64>();
        tracing::span::Id::from_u64(id)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {
        // Record additional values to span
        // For now, we don't store span state
    }

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {
        // Handle follows_from relationships
    }

    fn event(&self, event: &tracing::Event<'_>) {
        let level = self.convert_tracing_level(*event.metadata().level());
        if level <= self.level_filter {
            let log_entry = self.event_to_log_entry(event);

            // Process through processor chain and transport
            match self.processor_chain.process(log_entry) {
                Ok(processed_entry) => {
                    if let Err(e) = self.transport.transport(&processed_entry) {
                        eprintln!("Tracing transport error: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Tracing processing error: {}", e);
                }
            }
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {
        // Enter span - could set context here
    }

    fn exit(&self, _span: &tracing::span::Id) {
        // Exit span - could clear context here
    }
}

/// Visitor to extract fields from tracing events
#[cfg(feature = "structured-logging")]
struct TracingFieldVisitor {
    fields: serde_json::Map<String, serde_json::Value>,
    message: Option<String>,
}

#[cfg(feature = "structured-logging")]
impl TracingFieldVisitor {
    fn new() -> Self {
        Self {
            fields: serde_json::Map::new(),
            message: None,
        }
    }
}

#[cfg(feature = "structured-logging")]
impl tracing::field::Visit for TracingFieldVisitor {
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::json!(value));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::json!(format!("{:?}", value)),
        );
    }
}

/// Builder for creating tracing-integrated logging setup
#[cfg(feature = "structured-logging")]
pub struct TracingIntegrationBuilder {
    processor_chain: Option<ProcessorChain>,
    transport: Option<Arc<dyn TransportPort>>,
    level_filter: LogLevel,
}

#[cfg(feature = "structured-logging")]
impl TracingIntegrationBuilder {
    pub fn new() -> Self {
        Self {
            processor_chain: None,
            transport: None,
            level_filter: LogLevel::Info,
        }
    }

    pub fn with_processor_chain(mut self, chain: ProcessorChain) -> Self {
        self.processor_chain = Some(chain);
        self
    }

    pub fn with_transport(mut self, transport: Arc<dyn TransportPort>) -> Self {
        self.transport = Some(transport);
        self
    }

    pub fn with_level_filter(mut self, level: LogLevel) -> Self {
        self.level_filter = level;
        self
    }

    pub fn build(self) -> ObservabilityResult<TracingSubscriberAdapter> {
        let processor_chain = self
            .processor_chain
            .unwrap_or_else(crate::domain::build_default_processor_chain);

        let transport = self
            .transport
            .unwrap_or_else(|| Arc::new(WasmStdoutAdapter::with_compact_formatter()));

        Ok(TracingSubscriberAdapter::new(
            processor_chain,
            transport,
            self.level_filter,
        ))
    }
}

#[cfg(feature = "structured-logging")]
impl Default for TracingIntegrationBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Basic WASM stdout adapter for metrics (correlation-focused)
pub struct WasmStdoutMetricsAdapter {
    enabled: bool,
}

impl WasmStdoutMetricsAdapter {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    pub fn disabled() -> Self {
        Self { enabled: false }
    }
}

impl Default for WasmStdoutMetricsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::ports::MetricsPort for WasmStdoutMetricsAdapter {
    fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        if !self.enabled {
            return Ok(());
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        println!(
            "[METRIC] {} counter {} value={} timestamp={}",
            timestamp, name, value, timestamp
        );
        Ok(())
    }

    fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        if !self.enabled {
            return Ok(());
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        println!(
            "[METRIC] {} histogram {} value={} timestamp={}",
            timestamp, name, value, timestamp
        );
        Ok(())
    }

    fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        if !self.enabled {
            return Ok(());
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        println!(
            "[METRIC] {} gauge {} value={} timestamp={}",
            timestamp, name, value, timestamp
        );
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Extended WASM adapter that handles both logs and metrics with correlation
pub struct UnifiedWasmStdoutAdapter {
    log_adapter: WasmStdoutAdapter,
    metrics_adapter: WasmStdoutMetricsAdapter,
}

impl UnifiedWasmStdoutAdapter {
    pub fn new() -> Self {
        Self {
            log_adapter: WasmStdoutAdapter::with_json_formatter(),
            metrics_adapter: WasmStdoutMetricsAdapter::new(),
        }
    }

    /// Process metrics entry through same formatting as logs for correlation
    pub fn transport_metric(&self, entry: &crate::domain::MetricsEntry) -> ObservabilityResult<()> {
        let json_output = entry.to_json();
        println!("[METRIC] {}", json_output);
        Ok(())
    }
}

impl Default for UnifiedWasmStdoutAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportPort for UnifiedWasmStdoutAdapter {
    fn transport(&self, entry: &LogEntry) -> ObservabilityResult<()> {
        self.log_adapter.transport(entry)
    }

    fn transport_batch(&self, entries: &[LogEntry]) -> ObservabilityResult<()> {
        self.log_adapter.transport_batch(entries)
    }
}

impl crate::ports::MetricsPort for UnifiedWasmStdoutAdapter {
    fn emit_counter_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        // Create a proper MetricsEntry for correlation
        let entry = crate::domain::create_counter_metric(name, value);
        self.transport_metric(&entry)
    }

    fn emit_histogram_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let entry = crate::domain::create_histogram_metric(name, value);
        self.transport_metric(&entry)
    }

    fn emit_gauge_simple(&self, name: &str, value: f64) -> ObservabilityResult<()> {
        let entry = crate::domain::create_gauge_metric(name, value);
        self.transport_metric(&entry)
    }

    fn is_enabled(&self) -> bool {
        self.metrics_adapter.is_enabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::create_log_entry;

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter;
        let entry = create_log_entry(
            LogLevel::Info,
            "test message",
            serde_json::json!({"key": "value"}),
        );

        let result = formatter.format(&entry).unwrap();
        assert!(result.contains("test message"));
        assert!(result.contains("Info"));
    }

    #[test]
    fn test_compact_formatter() {
        let formatter = CompactJsonFormatter;
        let entry = create_log_entry(
            LogLevel::Error,
            "error occurred",
            serde_json::json!({"error_code": 500}),
        );

        let result = formatter.format(&entry).unwrap();
        assert!(result.contains("[ERROR]"));
        assert!(result.contains("error occurred"));
    }

    #[test]
    fn test_wasm_stdout_adapter() {
        let adapter = WasmStdoutAdapter::with_json_formatter();
        let entry = create_log_entry(LogLevel::Debug, "debug info", serde_json::json!({}));

        // This would print to stdout in real usage
        assert!(adapter.transport(&entry).is_ok());
    }

    #[test]
    fn directives_simple_level() {
        let d = LogDirectives::parse("debug");
        assert_eq!(d.global_level(), LogLevel::Debug);
        assert_eq!(d.max_level(), LogLevel::Debug);
        assert!(d.enabled(LogLevel::Info, "anything"));
        assert!(d.enabled(LogLevel::Debug, "anything"));
        assert!(!d.enabled(LogLevel::Trace, "anything"));
    }

    #[test]
    fn directives_per_crate() {
        let d = LogDirectives::parse("info,agent_sdk=debug,a2a_protocol_core=trace");
        assert_eq!(d.global_level(), LogLevel::Info);
        assert_eq!(d.max_level(), LogLevel::Trace);

        assert!(d.enabled(LogLevel::Info, "some_crate"));
        assert!(!d.enabled(LogLevel::Debug, "some_crate"));

        assert!(d.enabled(LogLevel::Debug, "agent_sdk"));
        assert!(d.enabled(LogLevel::Debug, "agent_sdk::handler"));
        assert!(!d.enabled(LogLevel::Trace, "agent_sdk"));

        assert!(d.enabled(LogLevel::Trace, "a2a_protocol_core"));
        assert!(d.enabled(LogLevel::Trace, "a2a_protocol_core::rpc"));
    }

    #[test]
    fn directives_from_level() {
        let d = LogDirectives::from_level(LogLevel::Warn);
        assert!(d.enabled(LogLevel::Warn, "x"));
        assert!(!d.enabled(LogLevel::Info, "x"));
    }

    #[test]
    fn directives_empty_string() {
        let d = LogDirectives::parse("");
        assert_eq!(d.global_level(), LogLevel::Info);
    }

    #[test]
    fn directives_unknown_tokens_ignored() {
        let d = LogDirectives::parse("info,bad_token,agent_sdk=debug");
        assert_eq!(d.global_level(), LogLevel::Info);
        assert!(d.enabled(LogLevel::Debug, "agent_sdk"));
    }
}
