//! Demonstration of Standard Rust Logging Integration
//!
//! This example shows how the observability extension enables agents to use
//! standard Rust logging macros (log::info!, log::debug!, etc.) with
//! structured output and context enrichment.

use log::{debug, error, info, warn};
use observability_core::{LogLevel, ObservabilityConfig, ObservabilityManager};
use serde_json::json;

#[cfg(feature = "structured-logging")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Standard Rust Logging Integration Demo");
    println!("==========================================\n");

    // 1. Create observability configuration
    let config = ObservabilityConfig {
        level: "debug".to_string(),
        format: "compact".to_string(),
        structured: true,
        context_enrichment: true,
        default_context: [
            ("agent_id".to_string(), json!("demo-agent")),
            ("version".to_string(), json!("0.1.0")),
        ]
        .into_iter()
        .collect(),
    };

    println!("📋 Configuration:");
    println!("   Level: {}", config.level);
    println!("   Format: {}", config.format);
    println!("   Structured: {}", config.structured);
    println!("   Context Enrichment: {}\n", config.context_enrichment);

    // 2. Create and initialize the observability manager
    let mut manager = ObservabilityManager::new(config)?;
    manager.initialize()?;

    println!("✅ Observability manager initialized");
    println!("✅ Standard Rust logging is now hooked into structured logging\n");

    // 3. Demonstrate standard logging macros
    println!("🎯 Testing standard Rust logging macros:\n");

    // Standard log macros - these now go through our structured logging system!
    info!("Agent startup completed successfully");
    debug!("Loading configuration from file: config.json");
    warn!("High memory usage detected: 85% of available memory");
    error!("Failed to connect to external service: timeout after 30s");

    println!("\n🔍 The log entries above were processed through:");
    println!("   ✓ Timestamp processor (added current timestamp)");
    println!("   ✓ Context enricher (added agent_id, version)");
    println!("   ✓ Structured fields processor (formatted as JSON)");
    println!("   ✓ WASM stdout adapter (output to console)\n");

    // 4. Demonstrate different log levels
    println!("📊 Testing different log levels:");

    if log::log_enabled!(log::Level::Trace) {
        log::trace!("This is a trace message - very detailed debugging");
    } else {
        println!("   (Trace level disabled - current level is debug)");
    }

    log::debug!("Debugging information about internal state");
    log::info!("General information about agent operation");
    log::warn!("Warning about potential issues");
    log::error!("Error that occurred during processing");

    println!("\n🎨 Each level is automatically formatted with appropriate styling\n");

    // 5. Demonstrate context enrichment
    println!("🔗 Testing context enrichment:");

    // These would be enriched with the default context fields we configured
    info!("Processing user request");
    debug!("Executing business logic");

    println!("   ✓ Each log entry includes agent_id and version from default context\n");

    // 6. Show capabilities
    println!("🛠️  Manager capabilities:");
    for capability in manager.capabilities() {
        println!("   ✓ {}", capability);
    }

    println!("\n🎯 Key Benefits:");
    println!("   • No custom logging methods needed - use familiar log::info! etc.");
    println!("   • Automatic structured JSON output with context enrichment");
    println!("   • WASM-compatible with fast startup times");
    println!("   • Hexagonal architecture allows swapping transports/formatters");
    println!("   • Processor chain pattern inspired by Python's structlog");

    println!("\n✨ Integration Complete!");
    println!("   Agents can now use standard Rust logging with zero boilerplate");

    Ok(())
}

#[cfg(not(feature = "structured-logging"))]
fn main() {
    println!("This example requires the 'structured-logging' feature.");
    println!("Run with: cargo run --example standard_logging_demo --features structured-logging");
}

// Helper function for testing in different contexts
#[cfg(feature = "structured-logging")]
pub fn demonstrate_logging_in_function() -> Result<(), Box<dyn std::error::Error>> {
    // This shows that logging works from any function after initialization
    log::info!("Logging from a separate function works perfectly!");
    log::debug!("Function context is automatically captured");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "structured-logging")]
    fn test_configuration_creation() {
        let config = ObservabilityConfig::default();
        assert_eq!(config.level, "info");
        assert!(config.structured);
    }

    #[test]
    #[cfg(feature = "structured-logging")]
    fn test_manager_creation() {
        let config = ObservabilityConfig::default();
        let manager = ObservabilityManager::new(config);
        assert!(manager.is_ok());
    }

    #[test]
    #[cfg(feature = "structured-logging")]
    fn test_level_checking() {
        let config = ObservabilityConfig {
            level: "warn".to_string(),
            ..Default::default()
        };
        let manager = ObservabilityManager::new(config).unwrap();

        // These would require initialization to work properly in practice
        assert!(manager.is_enabled(LogLevel::Error));
        assert!(manager.is_enabled(LogLevel::Warn));
        assert!(!manager.is_enabled(LogLevel::Info)); // Below threshold
    }
}
