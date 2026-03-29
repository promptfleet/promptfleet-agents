//! # Enhanced Structured Logging for SpinKube Agents
//!
//! This crate provides performance optimizations and convenience APIs on top of the excellent
//! `observability_core` hexagonal architecture. It focuses on:
//!
//! - 🚀 **Performance**: String interning, buffer pooling, zero-allocation fast paths
//! - 🔗 **Correlation**: Enhanced W3C baggage, scoped contexts, advanced tracing
//! - 🎪 **Convenience**: Domain-specific APIs for LLM, template, and A2A operations
//!
//! ## Architecture
//!
//! This crate builds on `observability_core`'s solid foundation:
//! - **Domain**: Reuses LogEntry, ProcessorChain, TraceContext
//! - **Ports**: Extends TransportPort, FormatterPort interfaces  
//! - **Adapters**: Enhances with performance-optimized implementations
//! - **Extension**: Provides enhanced configuration and management
//!
//! ## Usage Tiers
//!
//! ### Tier 1: Standard (90% of users)
//! ```rust,ignore
//! // Just use observability_core - works perfectly
//! use observability_core::ObservabilityManager;
//! log::info!("Standard structured logging");
//! ```
//!
//! ### Tier 2: Enhanced (8% of users)  
//! ```rust,ignore
//! // Add convenience methods
//! use structured_logging::convenience::*;
//! log_llm_request("gpt-4", duration, tokens, "success")?;
//! ```
//!
//! ### Tier 3: Performance (2% of users)
//! ```rust,ignore
//! // Full performance optimization
//! use structured_logging::{EnhancedObservabilityConfig, PerformanceExtension};
//! ```

// 🚀 PERFORMANCE ENHANCEMENTS
#[cfg(feature = "performance-optimized")]
pub mod performance;

// 🔗 CORRELATION ENHANCEMENTS
#[cfg(feature = "correlation-enhanced")]
pub mod correlation;

// 🎪 CONVENIENCE APIs
#[cfg(feature = "convenience")]
pub mod convenience;

// 🔌 CONTEXT ADAPTER for observability_core integration
#[cfg(feature = "convenience")]
pub mod context_adapter;

// 🛡️ PANIC HANDLING
pub mod panic_handler;

// 🧩 ENHANCED CONFIGURATION AND MANAGEMENT
pub mod extension;

// Error types
mod error;

// 🏛️ RE-EXPORT FOUNDATION - Use observability_core as the source of truth
pub use observability_core::{
    BatchingConfig,
    BatchingManager,
    CompactJsonFormatter,
    ContextPort,
    FormatterPort,
    JsonFormatter,
    // Core domain types
    LogEntry,
    ObservabilityConfig,

    // Error handling
    ObservabilityError,
    // Core framework
    ObservabilityManager,
    ObservabilityResult,

    ProcessorChain,
    StandardLogAdapter,

    StandardLoggingPort,
    // Ports and adapters
    TransportPort,
    // Utilities
    W3CTraceContext,
    WasmStdoutAdapter,
    traits::LogLevel,
};

// Re-export the correct TraceContext from domain
pub use observability_core::domain::TraceContext;

// 🎯 ENHANCED FUNCTIONALITY
pub use error::{Result, StructuredLoggingError};
pub use extension::{
    EnhancedObservabilityConfig, PerformanceExtension, create_performance_extension,
    create_performance_extension_from_config,
};

// 🛡️ PANIC HANDLING EXPORTS
pub use panic_handler::{
    PanicHandlerConfig, PanicPattern, PanicSeverity, PanicStats, StructuredPanicInfo, ThreadInfo,
    get_panic_stats, install_panic_handler, install_panic_handler_with_config, reset_panic_stats,
};

// 🛡️ The `supervised` macro is available at crate root due to #[macro_export]
// No need to re-export it - it's automatically available as structured_logging::supervised!

// 🚀 PERFORMANCE EXPORTS (feature-gated)
#[cfg(feature = "performance-optimized")]
pub use performance::{
    BufferPool, FastPathLogger, OptimizedWasmStdoutAdapter, PerformanceStats,
    StringInterningProcessor,
};

// 🔗 CORRELATION EXPORTS (feature-gated)
#[cfg(feature = "correlation-enhanced")]
pub use correlation::{
    BaggageManager, CorrelationProcessor, EnhancedTraceContext, ScopedContextManager,
    W3CBaggageSupport,
};

// 🎪 CONVENIENCE EXPORTS (feature-gated)
#[cfg(feature = "convenience")]
pub use convenience::{
    A2AContext,
    ConvenienceManager,
    // Processors
    DomainContextProcessor,
    // Context types
    LLMContext,
    TemplateContext,
    clear_a2a_context,
    clear_all_contexts,
    clear_llm_context,
    clear_request_context,
    clear_template_context,
    emit_a2a_message_latency,
    emit_counter,
    emit_histogram,
    // Metrics convenience functions
    emit_llm_request_duration,
    emit_llm_tokens_used,
    emit_request_duration,
    emit_template_render_duration,
    log_a2a_message,
    // Convenience logging functions
    log_llm_request,
    log_template_render,
    set_a2a_context,
    // Context management
    set_llm_context,
    set_request_context,
    set_template_context,
};

// 🔌 ADVANCED CONTEXT MANAGEMENT EXPORTS (feature-gated)
#[cfg(feature = "convenience")]
pub use context_adapter::{
    A2aContextGuard,
    A2aContextManager,
    AllContextsGuard,

    ContextManagerRegistry,
    // RAII guards
    LlmContextGuard,
    // Traits (for advanced users)
    LlmContextManager,
    RequestContextGuard,
    RequestContextManager,
    // Advanced builder
    ScopedContextBuilder,

    StructuredA2aContextManager,
    // Registry and managers
    StructuredContextRegistry,
    StructuredLlmContextManager,
    StructuredRequestContextManager,
    // Initialization
    init_context_integration,

    set_a2a_context_scoped,
    set_all_contexts_scoped,

    // Scoped context setting
    set_llm_context_scoped,
    set_request_context_scoped,
    with_a2a_context,
    with_all_contexts,

    // Scoped callbacks
    with_llm_context,
    with_request_context,
};

// Re-export configuration types
#[cfg(feature = "convenience")]
pub use extension::ConvenienceConfig;
#[cfg(feature = "correlation-enhanced")]
pub use extension::CorrelationConfig;
#[cfg(feature = "performance-optimized")]
pub use extension::PerformanceConfig;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Create a logger using observability_core foundation
pub fn create_logger() -> ObservabilityResult<ObservabilityManager> {
    ObservabilityManager::new(ObservabilityConfig::default())
}

/// Create an enhanced logger with performance optimizations
#[cfg(feature = "performance-optimized")]
pub fn create_performance_logger() -> Result<PerformanceExtension> {
    PerformanceExtension::new(EnhancedObservabilityConfig::default())
}

/// Create logger from configuration
pub fn create_logger_from_config(
    config: ObservabilityConfig,
) -> ObservabilityResult<ObservabilityManager> {
    ObservabilityManager::new(config)
}

/// Create enhanced logger from enhanced configuration
pub fn create_enhanced_logger_from_config(
    config: EnhancedObservabilityConfig,
) -> Result<PerformanceExtension> {
    PerformanceExtension::new(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_logger_creation() {
        let logger = create_logger();
        assert!(logger.is_ok());
    }

    #[cfg(feature = "performance-optimized")]
    #[test]
    fn test_performance_logger_creation() {
        let logger = create_performance_logger();
        assert!(logger.is_ok());
    }

    #[test]
    fn test_version_constant() {
        assert!(!VERSION.is_empty());
    }
}
