//! Simple Panic Handling Demo for SpinKube Agents

use structured_logging::{
    get_panic_stats, supervised, EnhancedObservabilityConfig, PerformanceExtension,
    StructuredLoggingError,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🛡️ Panic Handling Demo for SpinKube Agents");
    println!("==========================================\n");

    // Create enhanced configuration with panic handling (this installs the panic handler)
    println!("🧩 Demonstrating integration with structured logging:");
    let enhanced_config = EnhancedObservabilityConfig::default();
    let _extension = PerformanceExtension::new(enhanced_config)?;
    println!("   ✅ Enhanced extension with panic handling created");

    // Demonstrate structured panic logging (simulated)
    println!("\n🔬 Demonstrating structured panic logging:");
    log::error!("🔥 Simulated structured panic (would be logged automatically on real panic)");

    // Demonstrate supervised execution
    println!("\n🛡️ Demonstrating supervised execution:");
    let safe_result = supervised!({
        println!("   Executing potentially dangerous operation...");
        "operation completed safely"
    });

    match safe_result {
        Ok(result) => println!("   ✅ Supervised operation succeeded: {}", result),
        Err(e) => println!("   🛡️ Supervised operation caught panic: {}", e),
    }

    // Try to get panic statistics
    match get_panic_stats() {
        Ok(stats) => {
            println!("\n📊 Panic Statistics:");
            println!("   Total panics: {}", stats.total_panics);
            println!(
                "   Panic patterns detected: {}",
                stats.common_patterns.len()
            );
            println!("   Last panic: {:?}", stats.last_panic_timestamp);
        }
        Err(e) => {
            println!("\nℹ️  Panic statistics: {}", e);
        }
    }

    // Show panic handler capabilities
    println!("\n💡 Panic Handler Capabilities:");
    println!("   🔥 Structured panic logging with full context");
    println!("   📊 Panic analytics and pattern detection");
    println!("   🛡️ Supervised execution for critical operations");
    println!("   🔄 Recovery mechanisms and self-healing");
    println!("   🎯 WASM-compatible implementation");
    println!("   🔗 Full integration with observability infrastructure");

    println!("\n🎉 Panic handling demo completed successfully!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panic_handling_demo() {
        // Test that the demo components can be created
        let enhanced_config = EnhancedObservabilityConfig::default();
        let _extension = PerformanceExtension::new(enhanced_config);
        assert!(_extension.is_ok());
    }

    #[test]
    fn test_supervised_macro() {
        let result = supervised!({ "test_value" });
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test_value");
    }
}
