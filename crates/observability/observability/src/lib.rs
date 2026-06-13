//! # Observability (Facade)
//!
//! This crate is the **single happy-path entrypoint** for PromptFleet observability.
//! It installs global structured logging once (via `observability_core`) and provides
//! a shareable `Obs` handle that subcomponents (A2A client/server, LLM client, etc.)
//! can accept as `Arc<dyn ObsHandle>`.
//!
//! Design goals:
//! - One initializer (`Obs::init`) per process
//! - One handle (`Obs`) passed into subcomponents
//! - Backend-agnostic usage (OTEL / Prometheus are optional, best-effort)

mod mesh_identity;
mod semconv;
mod service_standard;
mod types;
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "axum")]
mod axum_service;

#[cfg(feature = "axum")]
pub use axum_service::*;
pub use mesh_identity::*;
#[cfg(feature = "runtime")]
pub use runtime::*;
pub use semconv::*;
pub use service_standard::*;
pub use types::{
    Obs, ObsHandle, ObsHealth, ObsResult, ObservabilityConfig, OtelConfig, OtelSampling,
    PrometheusConfig, SharedObs,
};

// Re-export core context helpers for convenience.
pub use observability_core::{
    ContextFuture, HeaderExtractor, HeaderInjector, TraceContext, W3CTraceContext,
    clear_current_context, get_current_context, set_current_context, with_context,
    with_context_future,
};

// Re-export core logging config (used as the `logging` field of `ObservabilityConfig`).
pub use observability_core::ObservabilityConfig as CoreLoggingConfig;

// Re-export core span/status/log level for interoperability.
pub use observability_core::traits::LogLevel;
pub use observability_core::{SpanGuard, SpanStatus};
