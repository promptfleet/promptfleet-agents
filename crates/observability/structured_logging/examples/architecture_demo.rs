//! Architecture Demo: Core Foundation vs Advanced Features
//!
//! This example demonstrates the proper separation between:
//! - `observability_core`: Basic trace context foundation
//! - `structured_logging`: Advanced domain-specific context management

use std::collections::HashMap;
use web_time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🏛️ Architecture Demo: Core vs Advanced Context Management");
    println!("=========================================================\n");

    // ==================== CORE FOUNDATION ====================
    println!("📚 CORE FOUNDATION (observability_core)");
    println!("---------------------------------------");

    // Basic W3C trace context from core
    let w3c_context = observability_core::context::W3CTraceContext::new_root();
    println!("✅ W3C Trace Context: {}", w3c_context);

    // Basic trace context from core
    let trace_context = observability_core::context::TraceContext::new_root();
    println!(
        "✅ Basic Trace Context: trace_id={}",
        trace_context.trace_id
    );

    // Thread-local context management from core
    observability_core::context::set_current_context(trace_context.clone());
    let retrieved = observability_core::context::get_current_context().unwrap();
    println!("✅ Thread-local context: trace_id={}", retrieved.trace_id);

    // Basic scoped context from core
    let result = observability_core::context::with_context(trace_context.clone(), || {
        let current = observability_core::context::get_current_context().unwrap();
        format!("Executing with trace_id: {}", current.trace_id)
    });
    println!("✅ Scoped context: {}", result);

    // Header propagation from core
    let mut headers = HashMap::new();
    let mut injector = observability_core::context::HeaderInjector(&mut headers);
    injector.inject(&w3c_context);
    println!("✅ W3C headers: {:?}", headers);

    println!(
        "💡 Core provides: Basic W3C, TraceContext, thread-local storage, header propagation\n"
    );

    // ==================== ADVANCED FEATURES ====================
    println!("🚀 ADVANCED FEATURES (structured_logging)");
    println!("-----------------------------------------");

    // Initialize advanced context management
    structured_logging::context_adapter::init_context_integration();
    println!("✅ Advanced context integration initialized");

    // Domain-specific RAII context management
    println!("\n🔒 RAII Context Management:");
    {
        let _llm_guard = structured_logging::set_llm_context_scoped("gpt-4", "openai_client");
        let _a2a_guard = structured_logging::set_a2a_context_scoped(
            "chat_completion",
            "agent-1",
            "agent-2",
            "a2a_server",
        );
        let _request_guard = structured_logging::set_request_context_scoped(
            "req-123",
            Some("user-456"),
            Some("session-789"),
        );

        println!("   ✅ All contexts set with RAII guards");
        log::info!("This log has LLM, A2A, and request context automatically");
    } // All contexts automatically cleared when guards drop
    println!("   ✅ All contexts automatically cleared");

    // Scoped callback APIs
    println!("\n📦 Scoped Callback APIs:");
    let llm_result = structured_logging::with_llm_context("gpt-3.5-turbo", "openai_client", || {
        log::info!("Processing LLM request with automatic context");
        "LLM processing complete"
    });
    println!("   ✅ LLM scoped callback: {}", llm_result);

    let a2a_result = structured_logging::with_a2a_context(
        "rpc_call",
        "agent-alpha",
        "agent-beta",
        "a2a_client",
        || {
            log::info!("Processing A2A message with automatic context");
            "A2A processing complete"
        },
    );
    println!("   ✅ A2A scoped callback: {}", a2a_result);

    // Advanced scoped context builder
    println!("\n🏗️ Advanced Context Builder:");
    let builder_result = structured_logging::ScopedContextBuilder::new()
        .with_llm_context("claude-3", "anthropic_client")
        .with_a2a_context("tool_call", "orchestrator", "worker", "a2a_rpc")
        .with_request_context("req-999", Some("user-888"), None)
        .execute(|| {
            log::info!("Complex operation with multiple contexts");
            "Multi-context operation complete"
        });
    println!("   ✅ Builder pattern: {}", builder_result);

    // Convenience macros (if enabled)
    println!("\n🎪 Convenience Macros:");
    structured_logging::with_llm_context_scoped!("gpt-4", "openai_proxy" => {
        log::info!("Using convenience macro for LLM context");
    });
    println!("   ✅ Macro-based context management");

    println!("💡 Advanced features: RAII guards, domain contexts, builders, macros\n");

    // ==================== INTEGRATION DEMO ====================
    println!("🔗 INTEGRATION DEMO");
    println!("------------------");

    // Show how advanced features build on core foundation
    let core_trace = observability_core::context::TraceContext::new_root();
    println!("✅ Created core trace context: {}", core_trace.trace_id);

    // Use advanced structured logging with the core trace context
    observability_core::context::set_current_context(core_trace.clone());

    structured_logging::with_all_contexts(
        // LLM context
        "gpt-4",
        "openai_client",
        // A2A context
        "chat_completion",
        "user-agent",
        "llm-agent",
        "a2a_server",
        // Request context
        "req-final",
        Some("user-final"),
        Some("session-final"),
        // Closure
        || {
            let current_trace = observability_core::context::get_current_context().unwrap();
            log::info!(
                "Integrated operation using core trace: {}",
                current_trace.trace_id
            );
        },
    );

    println!("✅ Successfully integrated core foundation with advanced features");

    println!("\n🎉 Architecture Demo completed successfully!");
    println!("\n📋 ARCHITECTURE SUMMARY:");
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│ 🏛️ OBSERVABILITY_CORE (Foundation)                          │");
    println!("│ ✅ W3C Trace Context                                        │");
    println!("│ ✅ Basic TraceContext                                       │");
    println!("│ ✅ Thread-local storage                                     │");
    println!("│ ✅ Header injection/extraction                              │");
    println!("│ ✅ Basic scoped context                                     │");
    println!("├─────────────────────────────────────────────────────────────┤");
    println!("│ 🚀 STRUCTURED_LOGGING (Advanced)                            │");
    println!("│ ✅ Domain-specific contexts (LLM, A2A, Request)             │");
    println!("│ ✅ RAII guards for automatic cleanup                        │");
    println!("│ ✅ Scoped callback APIs                                     │");
    println!("│ ✅ Advanced context builder pattern                         │");
    println!("│ ✅ Convenience macros                                       │");
    println!("│ ✅ Dependency injection registry                            │");
    println!("└─────────────────────────────────────────────────────────────┘");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_architecture_separation() {
        // Core functionality works independently
        let w3c_context = observability_core::context::W3CTraceContext::new_root();
        assert!(!w3c_context.trace_id.is_empty());

        let trace_context = observability_core::context::TraceContext::new_root();
        assert!(!trace_context.trace_id.is_empty());

        // Advanced features work with core foundation
        structured_logging::context_adapter::init_context_integration();

        let result =
            structured_logging::with_llm_context("test-model", "test-component", || "test-result");
        assert_eq!(result, "test-result");
    }

    #[test]
    fn test_core_and_advanced_integration() {
        // Create core trace context
        let core_trace = observability_core::context::TraceContext::new_root();
        let trace_id = core_trace.trace_id.clone();

        // Set core context
        observability_core::context::set_current_context(core_trace);

        // Initialize advanced features
        structured_logging::context_adapter::init_context_integration();

        // Use advanced features with core context
        let result = structured_logging::with_llm_context("test-model", "test-component", || {
            let current = observability_core::context::get_current_context().unwrap();
            current.trace_id
        });

        assert_eq!(result, trace_id);
    }
}
