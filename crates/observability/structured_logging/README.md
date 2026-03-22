# Structured Logging - Enhanced Performance Extension

> **Performance-optimized structured logging extension built on observability_core foundation**

This crate provides enhanced performance optimizations, convenience APIs, and advanced features on top of the excellent `observability_core` hexagonal architecture.

## 🎯 **Core Mission**

Provide enhanced structured logging with performance optimizations and convenience APIs:
```rust
let agent = AgentBuilder::from_config("config.json")?.init()?;
// Enhanced structured logging with convenience methods
set_llm_context("gpt-4", "chat_completion", Some(120), Some("success"));
log::info!("LLM request completed");
```

## 🏗️ **Architecture**

Built on `observability_core` foundation with enhanced layers:
```
📦 structured_logging/
├── 🚀 performance/     # String interning, buffer pooling, zero-allocation fast paths
├── 🔗 correlation/     # Enhanced W3C baggage, scoped contexts, advanced tracing
├── 🎪 convenience/     # Domain-specific APIs (LLM, template, A2A operations)
├── 🔌 context_adapter/ # RAII context management with dependency injection
├── 🛡️ panic_handler/   # Structured panic handling and recovery
└── 🧩 extension/       # Enhanced configuration and auto-discovery
```

## ✨ **Enhanced Features**

### 🚀 **Performance Optimizations**
- ✅ **String Interning**: Zero-allocation for repeated log field values
- ✅ **Buffer Pooling**: Reusable buffer management for memory efficiency
- ✅ **Fast Path Logging**: Optimized hot-path for high-frequency logging
- ✅ **Performance Statistics**: Hit ratios and performance monitoring
- ✅ **WASM-Optimized**: Memory management tuned for WebAssembly

### 🔗 **Enhanced Correlation**
- ✅ **W3C Baggage Support**: Extended baggage handling beyond basic trace context
- ✅ **Scoped Context Management**: Advanced context scoping with automatic cleanup
- ✅ **Correlation Processor**: Enhanced correlation between different telemetry types
- ✅ **Enhanced TraceContext**: Extended trace context with additional metadata

### 🎪 **Convenience APIs**
- ✅ **Domain Context Management**: LLM, Template, A2A context helpers
- ✅ **Context Setting Functions**: `set_llm_context()`, `set_template_context()`, `set_a2a_context()`
- ✅ **Context Clearing Functions**: `clear_llm_context()`, `clear_all_contexts()`
- ✅ **Metrics Convenience**: `emit_llm_request_duration()`, `emit_a2a_message_latency()`
- ✅ **Domain Context Processor**: Automatic context enrichment for domain operations

### 🔌 **Advanced Context Management**
- ✅ **RAII Context Guards**: Exception-safe context management with automatic cleanup
- ✅ **Scoped Context Builders**: Fluent API for complex context setups
- ✅ **Context Registry**: Centralized context management system
- ✅ **Scoped Callbacks**: `with_llm_context()`, `with_a2a_context()` helper functions
- ✅ **Context Managers**: Specialized managers for different context types

### 🛡️ **Panic Handling**
- ✅ **Structured Panic Info**: Rich panic information capture
- ✅ **Panic Statistics**: Track panic patterns and frequencies
- ✅ **Thread Information**: Capture thread context during panics
- ✅ **Supervised Macro**: `supervised!{}` macro for panic-safe code blocks
- ✅ **Recovery Strategies**: Configurable panic handling strategies

### 🧩 **Enhanced Extension**
- ✅ **Enhanced Configuration**: Extended config options beyond core
- ✅ **Performance Extension**: Specialized extension with performance features
- ✅ **Auto-Discovery**: Full `component_core::Extension` trait implementation
- ✅ **Multi-Config Support**: Support for convenience, performance, and correlation configs

## 📋 **Feature Flags**

```toml
[features]
default = ["convenience"]
convenience = ["observability_core/structured-logging", "serde", "serde_json"]
performance-optimized = ["convenience", "dashmap", "parking_lot"]
correlation-enhanced = ["convenience", "uuid"]
```

## 🚀 **Quick Start**

### 1. Add to Cargo.toml
```toml
[dependencies]
sdk = { workspace = true }
structured_logging = { workspace = true }
```

### 2. Configure in config.json
```json
{
  "extensions": {
    "structured_logging": {
      "enabled": true,
      "format": "json",
      "include_timestamp": true,
      "include_correlation_id": true,
      "log_level": "info",
      "output_target": "stdout"
    }
  }
}
```

### 3. Use Enhanced Features
```rust
use sdk::AgentBuilder;
use structured_logging::{
    set_llm_context, set_a2a_context, clear_all_contexts,
    with_llm_context, supervised
};
use log::info;

fn main() -> ComponentResult<()> {
    let agent = AgentBuilder::from_config("config.json")?.init()?;
    
    // Set LLM context for enhanced logging
    set_llm_context("gpt-4", "chat_completion", Some(120), Some("success"));
    info!("LLM request started");
    
    // Use scoped context management
    with_llm_context("claude-3", "analysis", Some(200), None, || {
        info!("Analysis request processing");
        // Context automatically cleared when scope ends
    });
    
    // Use supervised macro for panic safety
    let result = supervised! {
        risky_operation()
    };
    
    Ok(())
}
```

## 📊 **Configuration Options**

### Enhanced Configuration Types
```rust
// Convenience features configuration
#[derive(Serialize, Deserialize)]
pub struct ConvenienceConfig {
    pub domain_contexts_enabled: bool,
    pub metrics_convenience_enabled: bool,
    pub auto_context_cleanup: bool,
}

// Performance optimizations configuration
#[derive(Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub string_interning_enabled: bool,
    pub buffer_pooling_enabled: bool,
    pub fast_path_threshold: usize,
    pub performance_monitoring: bool,
}

// Enhanced correlation configuration
#[derive(Serialize, Deserialize)]
pub struct CorrelationConfig {
    pub w3c_baggage_enabled: bool,
    pub enhanced_trace_context: bool,
    pub correlation_processor_enabled: bool,
}
```

### Panic Handler Configuration
```rust
#[derive(Serialize, Deserialize)]
pub struct PanicHandlerConfig {
    pub severity: PanicSeverity,           // Critical, High, Medium, Low
    pub capture_thread_info: bool,
    pub capture_backtrace: bool,
    pub structured_output: bool,
    pub panic_stats_enabled: bool,
}
```

## ✅ **Verification Success Criteria**

### 🧪 **Unit Tests**
- [x] **Convenience APIs**: All context setting/clearing functions work correctly
- [x] **Performance Features**: String interning and buffer pooling function properly
- [x] **Correlation Features**: Enhanced trace context and baggage support work
- [x] **Panic Handling**: Structured panic capture and recovery work correctly
- [x] **Configuration Tests**: All enhanced config options parse and validate
- [x] **RAII Guards**: Context guards properly clean up on scope exit

### 🔗 **Integration Tests**
- [ ] **Extension Discovery**: Auto-discovery as `"structured_logging"` extension works
- [ ] **Context Propagation**: Enhanced contexts propagate correctly through call chains
- [ ] **Performance Integration**: Performance optimizations integrate with observability_core
- [ ] **Convenience Integration**: Domain-specific APIs enhance standard logging
- [ ] **Panic Integration**: Panic handler integrates with logging system
- [ ] **Multi-Feature Integration**: All feature flags work together correctly

### 🎯 **End-to-End Tests with real_world_verification**
- [ ] **Agent Lifecycle**: Full agent startup with enhanced logging features
- [ ] **LLM Context Usage**: LLM context APIs work in real agent scenarios
- [ ] **A2A Context Usage**: A2A context APIs work in agent communication
- [ ] **Template Context Usage**: Template context APIs work with rendering
- [ ] **Performance Under Load**: Performance optimizations work under sustained load
- [ ] **Error Scenarios**: Enhanced error handling works in failure scenarios

### 📊 **Performance Benchmarks**
- [ ] **String Interning Efficiency**: >90% hit ratio for repeated strings
- [ ] **Buffer Pool Efficiency**: <50% allocation overhead compared to naive approach
- [ ] **Fast Path Performance**: <5ns overhead for fast-path logging
- [ ] **Memory Efficiency**: <2MB additional memory footprint for all features
- [ ] **Context Overhead**: <30ns overhead for enhanced context management

### 🔍 **Feature Validation**
- [ ] **Convenience APIs**: All domain-specific context APIs work correctly
- [ ] **Performance Features**: String interning and buffer pooling provide measurable benefits
- [ ] **Correlation Features**: Enhanced correlation works across telemetry types
- [ ] **Panic Handling**: Structured panic information captured correctly
- [ ] **RAII Context Management**: Context guards prevent leaks and ensure cleanup

### 🚀 **Production Readiness**
- [ ] **Graceful Fallback**: Falls back to observability_core if enhanced features fail
- [ ] **Resource Management**: All enhanced features respect WASM memory constraints
- [ ] **Error Recovery**: Enhanced features don't break core logging functionality
- [ ] **Performance Stability**: Performance optimizations don't introduce instability
- [ ] **Multi-Agent Compatibility**: Works correctly with multiple agent instances

## 📚 **Examples**

### Available Examples
- **`architecture_demo.rs`**: Demonstrates layered architecture with observability_core
- **`comprehensive_demo.rs`**: Complete feature demonstration
- **`context_aware_demo.rs`**: Advanced context management patterns
- **`metrics_convenience_demo.rs`**: Convenience APIs for metrics
- **`panic_handling_demo.rs`**: Structured panic handling demonstration
- **`phase3_integration_test.rs`**: Full integration test example
- **`simple_demo.rs`**: Basic enhanced logging usage

### Running Examples
```bash
# Basic enhanced logging
cargo run --example simple_demo --features convenience

# Comprehensive feature demo
cargo run --example comprehensive_demo --features convenience,performance-optimized

# Context management demo
cargo run --example context_aware_demo --features convenience

# Performance optimization demo
cargo run --example architecture_demo --features performance-optimized

# Panic handling demo
cargo run --example panic_handling_demo
```

## 🛠️ **Development & Testing**

### Run Tests
```bash
# Basic tests
cargo test

# All features
cargo test --features convenience,performance-optimized,correlation-enhanced

# Performance tests
cargo test --release --features performance-optimized
```

### Verify Performance Optimizations
```bash
# String interning benchmarks
cargo test test_string_interning_performance --release -- --nocapture

# Buffer pool benchmarks  
cargo test test_buffer_pool_efficiency --release -- --nocapture
```

## 🎯 **Usage Patterns**

### Basic Enhanced Logging
```rust
use structured_logging::{set_llm_context, clear_all_contexts};

// Set context for LLM operations
set_llm_context("gpt-4", "chat_completion", Some(150), Some("success"));
log::info!("LLM request completed");
clear_all_contexts();
```

### RAII Context Management
```rust
use structured_logging::{set_llm_context_scoped, LlmContextGuard};

// Context automatically cleared when guard drops
let _guard = set_llm_context_scoped("claude-3", "analysis", Some(200), None);
log::info!("Analysis in progress");
// Context automatically cleared here
```

### Scoped Operations
```rust
use structured_logging::with_llm_context;

// Context scoped to the closure
with_llm_context("gpt-4", "completion", Some(100), None, || {
    log::info!("Processing request");
    // Complex operation here
    // Context automatically cleared when closure returns
});
```

### Panic-Safe Operations
```rust
use structured_logging::supervised;

let result = supervised! {
    potentially_panicking_operation()
};

match result {
    Ok(value) => log::info!("Operation completed successfully"),
    Err(panic_info) => log::error!("Operation panicked: {:?}", panic_info),
}
```

---

**Enhanced** • **Performance-Optimized** • **Convenience APIs** • **Built on Foundation** 