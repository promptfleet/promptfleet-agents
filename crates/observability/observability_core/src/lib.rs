//! # Observability Core
//!
//! Core traits and foundational components for structured logging and observability.
//! This crate provides zero-cost abstractions for telemetry, metrics, and structured logging
//! that can be conditionally compiled based on feature flags.
//!
//! ## Features
//!
//! - `observability`: Enables all observability features
//! - `otel-2025`: OpenTelemetry 2025 integration
//! - `structured-logging`: JSON structured logging support
//! - `prometheus-federation`: Prometheus metrics federation
//! - `auto-instrumentation`: Automatic instrumentation capabilities

pub mod batching;
pub mod context;
pub mod error;
pub mod noop;
pub mod traits;

// Core hexagonal architecture modules for standard logging integration
pub mod adapters;
pub mod domain;
pub mod extension;
pub mod ports;

// Re-export commonly used types
pub use batching::{BatchingConfig, BatchingManager};
pub use context::{
    clear_current_context,
    get_current_context,
    // Thread-local management
    set_current_context,
    with_context,
    HeaderExtractor,
    // Header utilities
    HeaderInjector,
    TraceContext,
    W3CTraceContext,
};
pub use error::{ObservabilityError, ObservabilityResult};
pub use noop::NoOpObservabilityPlugin;
pub use traits::{
    LogLevel, MetricsCollector, ObservabilityPlugin, SpanGuard, SpanStatus, StructuredLogger,
};

// Re-export hexagonal architecture types
pub use adapters::{
    CompactJsonFormatter, JsonFormatter, LogDirectives, StandardLogAdapter,
    UnifiedWasmStdoutAdapter, WasmStdoutAdapter, WasmStdoutMetricsAdapter,
};
#[cfg(feature = "structured-logging")]
pub use adapters::{TracingIntegrationBuilder, TracingSubscriberAdapter};
pub use domain::{
    create_counter_metric, create_gauge_metric, create_histogram_metric, BasicMetricType,
    EnhancedContextEnricher, LogEntry, LogKvExtractor, MetricsEntry, MetricsSource, ProcessorChain,
    TraceCorrelation,
};
pub use extension::{
    create_observability_manager, GlobalLoggerSingleton, ObservabilityConfig, ObservabilityManager,
};
pub use ports::{
    BatchingPort, ContextPort, FormatterPort, MetricsPort, StandardLoggingPort, TransportPort,
};

/// Version information for the observability core
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default batch size for telemetry data
pub const DEFAULT_BATCH_SIZE: usize = 100;

/// Default flush interval in seconds
pub const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 5;
