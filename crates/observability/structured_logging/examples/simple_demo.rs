//! Simple demonstration of structured_logging for SpinKube agents
//!
//! This example shows the basic usage of structured_logging without
//! complex feature gates, suitable for most SpinKube agents.

use serde_json::json;
use web_time::Duration;

use observability_core::traits::LogLevel;
use structured_logging::{EnhancedObservabilityConfig, PerformanceExtension, log_llm_request};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Simple Structured Logging Demo for SpinKube");
    println!("==============================================\n");

    // 1. Create enhanced configuration
    let config = EnhancedObservabilityConfig::default();
    println!("✅ Configuration created:");
    println!("   - Level: {}", config.base.level);
    println!("   - Format: {}", config.base.format);
    println!("   - Structured: {}", config.base.structured);

    // 2. Create enhanced extension
    let extension = PerformanceExtension::new(config)?;
    println!("✅ Enhanced logging extension created");
    println!(
        "   - Performance enabled: {}",
        extension.is_performance_enabled()
    );
    println!(
        "   - Correlation enabled: {}",
        extension.is_correlation_enabled()
    );
    println!(
        "   - Convenience enabled: {}",
        extension.is_convenience_enabled()
    );
    println!();

    // 3. Test convenience APIs
    println!("🎪 Testing Convenience APIs");
    println!("---------------------------");

    // LLM request logging
    let llm_entry = log_llm_request("gpt-4", Duration::from_millis(250), 1500, "success")?;
    println!("✅ LLM Request logged: {}", llm_entry.message);

    // Template logging (using convenience functions)
    let template_result = structured_logging::convenience::log_template_render(
        "Sailfish",
        "agent_card.stpl",
        Duration::from_millis(50),
        2048,
        "success",
    );
    if let Ok(entry) = template_result {
        println!("✅ Template render logged: {}", entry.message);
    }

    // A2A message logging
    let a2a_result = structured_logging::convenience::log_a2a_message(
        "chat_completion",
        "agent-1",
        "agent-2",
        Some(Duration::from_millis(100)),
        "success",
    );
    if let Ok(entry) = a2a_result {
        println!("✅ A2A message logged: {}", entry.message);
    }

    println!();

    // 4. Test standard Rust logging integration
    println!("📝 Testing Standard Rust Logging");
    println!("--------------------------------");

    // This is what most users will do - just use standard log macros
    log::info!("Agent started successfully");
    log::debug!("Processing request with ID: {}", "demo-123");
    log::warn!("This is a warning message");

    println!("✅ Standard Rust logging works with structured output");
    println!();

    // 5. Show configuration options
    println!("⚙️  Configuration Examples");
    println!("-------------------------");

    let mut debug_config = EnhancedObservabilityConfig::default();
    debug_config.base.level = "debug".to_string();
    debug_config.base.format = "json".to_string();

    println!("Debug config:");
    println!("   - Level: {}", debug_config.base.level);
    println!("   - Format: {}", debug_config.base.format);

    // Performance configuration
    #[cfg(feature = "performance-optimized")]
    {
        let perf_config = debug_config.with_all_performance_features();
        println!(
            "Performance features enabled: {}",
            perf_config.performance.enable_fast_paths
        );
    }

    println!();

    println!("🎉 Simple structured logging demo completed successfully!");
    println!("\n💡 Tips for SpinKube agents:");
    println!("   - Import structured_logging in your Cargo.toml");
    println!("   - Use log::info!(), log::debug!(), etc. for most logging");
    println!("   - Use convenience functions for domain-specific logging");
    println!("   - Enable performance features for high-throughput agents");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_demo_components() {
        // Test that all the components used in the demo work
        let config = EnhancedObservabilityConfig::default();
        assert_eq!(config.base.level, "info");

        let extension = PerformanceExtension::new(config);
        assert!(extension.is_ok());

        let llm_result = log_llm_request("gpt-4", Duration::from_millis(100), 1000, "success");
        assert!(llm_result.is_ok());
    }
}
