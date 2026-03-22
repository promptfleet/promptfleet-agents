//! Context-Aware Structured Logging Demo
//!
//! Demonstrates how to use standard log::info!, log::debug! macros with automatic
//! domain-specific enhancement through context-aware processors.

use structured_logging::*;

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Context-Aware Structured Logging Demo for SpinKube");
    println!("=====================================================\n");

    // Initialize enhanced extension with domain context processor
    let config = EnhancedObservabilityConfig::default();
    let extension = PerformanceExtension::new(config)?;
    println!("✅ Context-aware structured logging initialized");
    println!();

    // 🔬 DEMO 1: LLM Operations with Context
    println!("🔬 DEMO 1: LLM Operations with Standard Logging");
    println!("-----------------------------------------------");

    // Set LLM context - this affects subsequent log calls
    set_llm_context("gpt-4", "llm_client");

    // Now standard log calls are automatically enhanced!
    log::info!("Request started");
    log::info!("Request completed with 250ms duration, 1500 tokens, success status");
    log::debug!("Token processing finished with 100 input tokens, 1400 output tokens");

    // Clear context
    clear_all_contexts();

    println!("   ✅ Standard log::info! automatically enhanced with LLM structure");
    println!();

    // 🎨 DEMO 2: Template Operations with Context
    println!("🎨 DEMO 2: Template Operations with Standard Logging");
    println!("---------------------------------------------------");

    // Set template context
    set_template_context("Sailfish", "agent_card.stpl", "template_engine");

    // Standard logging with automatic template enhancement
    log::debug!("Template compilation started");
    log::info!("Template rendered in 50ms, size 2048 bytes, success");
    log::debug!("Template cache updated");

    clear_all_contexts();

    println!("   ✅ Standard log::info! automatically enhanced with template structure");
    println!();

    // 🔗 DEMO 3: A2A Operations with Context
    println!("🔗 DEMO 3: A2A Operations with Standard Logging");
    println!("----------------------------------------------");

    // Set A2A context
    set_a2a_context("chat_completion", "agent-1", "agent-2", "a2a_client");

    // Standard logging with automatic A2A enhancement
    log::info!("A2A message sent");
    log::info!("A2A response received in 100ms, success");
    log::debug!("A2A connection established");

    clear_all_contexts();

    println!("   ✅ Standard log::info! automatically enhanced with A2A structure");
    println!();

    // 📊 DEMO 4: Standard Logging Still Works
    println!("📊 DEMO 4: Standard Logging Still Works");
    println!("---------------------------------------");

    // Without any context, standard logging works normally
    log::info!("Agent startup completed");
    log::debug!("Configuration loaded from file: config.json");
    log::warn!("High memory usage detected: 85%");
    log::error!("Connection timeout to external service");

    println!("   ✅ Standard logging without context works as expected");
    println!();

    println!("🎉 Context-aware structured logging demo completed successfully!");
    println!();
    println!("💡 Key Benefits:");
    println!("   ✨ Use standard log::info!, log::debug! macros");
    println!("   🎯 Automatic domain-specific enhancement based on context");
    println!("   🔧 No custom methods to remember");
    println!("   📋 Built on hexagonal architecture foundation");
    println!("   ⚡ WASM-compatible and performant");

    Ok(())
}

/// Test the context-aware processor integration
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_aware_demo_components() {
        // Basic smoke test that the components can be created
        let config = EnhancedObservabilityConfig::default();
        let extension = PerformanceExtension::new(config);
        assert!(extension.is_ok());

        // Test context setting
        set_llm_context("test-model", "test-component");
        clear_all_contexts();

        // Test should pass if no panics occur
        assert!(true);
    }
}
