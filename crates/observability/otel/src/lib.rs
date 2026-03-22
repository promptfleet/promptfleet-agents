//! # OpenTelemetry Extension
//!
//! Advanced OpenTelemetry integration for structured logging and observability.
//! This extension provides comprehensive OTLP export, auto-instrumentation, and correlation
//! support for three-pillar observability.
//!
//! ## Features
//! - OTLP/HTTP export to OpenTelemetry Collector
//! - WASM-compatible async execution
//! - Auto-instrumentation for HTTP requests
//! - W3C Trace Context propagation  
//! - Sampling strategies
//! - Resource attribute management

pub mod auto_instrumentation;
pub mod collector_client;
pub mod resource_attributes;
pub mod sampling;

pub mod plugin;

// 🧩 CORE MODULE for standalone usage (extension system removed)
pub mod extension;

// Public API exports
pub use auto_instrumentation::{
    AutoInstrumentedHttpClient, FunctionInstrumentation, TraceContextPropagator,
};
pub use collector_client::{CollectorClient, LogData, MetricData, OtelSpanData, SpanEvent};
pub use plugin::{Otel, OtelBuilder, OtelConfig, OtelConfigBuilder};
pub use resource_attributes::ResourceAttributeManager;
pub use sampling::SamplingStrategy;

// 🧩 CORE EXPORTS for standalone usage (extension system removed)
pub use extension::{OtelExtensionConfig, OtelManager};

// Re-export observability core traits for integration
pub use observability_core::{
    LogLevel, ObservabilityPlugin, ObservabilityResult, SpanGuard, SpanStatus, TraceContext,
    W3CTraceContext,
};

// Constants
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default OTLP endpoint for local development
pub const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";

/// Default batch size for span export
pub const DEFAULT_BATCH_SIZE: usize = 512;

/// Default export timeout in seconds
pub const DEFAULT_EXPORT_TIMEOUT_SECS: u64 = 30;

/// Create a standalone OTEL manager
pub fn create_otel_manager() -> Result<OtelManager, String> {
    OtelManager::from_env()
}

/// Create an OTEL manager from configuration
pub fn create_otel_manager_from_config(config: OtelExtensionConfig) -> Result<OtelManager, String> {
    OtelManager::from_config(config)
}
