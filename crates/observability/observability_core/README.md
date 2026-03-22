# Observability Core - Foundation Library

> **WASM-native hexagonal observability foundation for structured logging**

This is the core observability library providing structured logging, metrics collection, and distributed tracing with pure WASM compatibility and zero external dependencies.

## 🎯 **Core Mission**

Provide solid observability foundation with standard Rust logging integration:
```rust
let manager = ObservabilityManager::new(config)?;
manager.initialize()?;
// Standard Rust logging is now structured and context-aware
log::info!("Processing request");
```

## 🏗️ **Hexagonal Architecture**

```
📦 observability_core/
├── 🧠 domain/          # Pure business logic (LogEntry, ProcessorChain, TraceContext)  
├── 🔌 ports/           # Abstract interfaces (TransportPort, FormatterPort, StandardLoggingPort)
├── 🔧 adapters/        # WASM implementations (WasmStdoutAdapter, StandardLogAdapter)
├── 🚀 extension/       # Core components (ObservabilityManager, GlobalLoggerSingleton)
├── 📊 batching/        # Telemetry batching system
├── 🌐 context/         # W3C trace context + thread-local management
└── 🎭 noop/           # Zero-cost no-op implementations
```

## ✨ **Core Features**

### 🔍 **Standard Logging Integration**
- ✅ **Rust log:: Integration**: `log::info!`, `log::debug!` work out of the box
- ✅ **Structured Fields**: Support for structured JSON output
- ✅ **Global Logger Singleton**: Thread-safe, initialize-once pattern
- ✅ **Context Enrichment**: Automatic trace_id, timestamp injection
- ✅ **Multiple Formatters**: JSON, Compact, PlainText formatters

### 📊 **Basic Metrics**
- ✅ **MetricsPort Interface**: Counter, Histogram, Gauge abstractions
- ✅ **MetricsEntry Domain**: Core metrics data structures
- ✅ **WASM Stdout Transport**: Stdout-based metrics export
- ✅ **Basic Types**: BasicMetricType enumeration (Counter, Histogram, Gauge)

### 🌐 **Distributed Tracing**
- ✅ **W3C Trace Context**: Full W3C traceparent/tracestate support
- ✅ **Header Injection/Extraction**: Automatic trace propagation
- ✅ **Thread-Local Context**: WASM-compatible context management  
- ✅ **Scoped Execution**: `with_context()` for guaranteed cleanup
- ✅ **TraceContext Domain**: Basic trace context management

### 🔧 **Processing Pipeline**
- ✅ **ProcessorChain**: Composable processing pipeline
- ✅ **Built-in Processors**: Timestamp, Context, StructuredFields, LevelFilter
- ✅ **LogProcessor Trait**: Interface for custom processors
- ✅ **Enhanced Context**: Context enrichment with metadata

### 📦 **Batching System**
- ✅ **BatchingManager**: Memory-efficient telemetry batching
- ✅ **BatchingConfig**: Configurable batch size and flush intervals
- ✅ **Multi-Type Support**: Logs and metrics batching
- ✅ **Buffer Statistics**: Real-time buffer monitoring

### 🎭 **No-Op Implementations**
- ✅ **Zero-Cost Abstractions**: Complete no-op implementations when disabled
- ✅ **NoOpTransport**: No-op transport for testing
- ✅ **NoOpProcessor**: No-op processor for benchmarking

### 🔌 **Core Integration**
- ✅ **Global Logger Singleton**: Thread-safe, initialize-once pattern
- ✅ **Configuration Management**: JSON/YAML config support with validation
- ✅ **Manager Interface**: Simple ObservabilityManager for easy integration
- ✅ **Factory Functions**: Convenient creation from configuration

## 📋 **Feature Flags**

```toml
[features]
default = ["structured-logging"]
observability = ["otel-2025", "structured-logging", "prometheus-federation"] 
otel-2025 = ["opentelemetry", "opentelemetry-semantic-conventions"]
structured-logging = ["serde", "serde_json", "chrono", "log", "tracing"]
prometheus-federation = ["prometheus"]
```

## 🚀 **Quick Start**

### 1. Add to Cargo.toml
```toml
[dependencies]
observability_core = { path = "../observability_core", features = ["structured-logging"] }
log = "0.4"
```

### 2. Create Configuration
```rust
use observability_core::{ObservabilityConfig, ObservabilityManager};

let config = ObservabilityConfig {
    level: "info".to_string(),
    format: "compact".to_string(),
    structured: true,
    context_enrichment: true,
    default_context: Default::default(),
};
```

### 3. Use in Your Application
```rust
use observability_core::{ObservabilityManager, ObservabilityResult};
use log::{info, debug, warn, error};

fn main() -> ObservabilityResult<()> {
    // Initialize observability manager
    let mut manager = ObservabilityManager::new(config)?;
    manager.initialize()?;
    
    // Standard Rust logging is now structured!
    info!("Application started successfully");
    debug!("Configuration loaded");
    warn!("High memory usage detected");
    
    Ok(())
}
```

## 📊 **Configuration**

### ObservabilityConfig
```rust
#[derive(Serialize, Deserialize)]
pub struct ObservabilityConfig {
    /// Log level: "error", "warn", "info", "debug", "trace"
    pub level: String,
    
    /// Output format: "json", "compact", "plain"  
    pub format: String,
    
    /// Enable structured logging features
    pub structured: bool,
    
    /// Enable context enrichment
    pub context_enrichment: bool,
    
    /// Default context fields for all log entries
    pub default_context: HashMap<String, serde_json::Value>,
}
```

### BatchingConfig
```rust
#[derive(Clone)]
pub struct BatchingConfig {
    /// Maximum items per batch (default: 100)
    pub max_batch_size: usize,
    
    /// Flush interval (default: 5s)
    pub flush_interval: Duration,
    
    /// Memory limit in bytes (default: 1MB)
    pub max_memory_bytes: usize,
    
    /// Drop on overflow vs evict oldest (default: true)
    pub drop_on_overflow: bool,
}
```

## ✅ **Verification Success Criteria**

### 🧪 **Unit Tests**
- [x] **Configuration Tests**: All config parsing and validation scenarios pass
- [x] **Processor Chain Tests**: All built-in processors work correctly
- [x] **Formatter Tests**: JSON, Compact, PlainText output validation passes
- [x] **Batching Tests**: Memory management and flush strategies work
- [x] **Context Tests**: W3C trace context and thread-local management
- [x] **No-Op Tests**: Zero-cost abstractions verified

### 🔗 **Integration Tests**  
- [ ] **Standard Logging**: `log::info!` produces structured JSON output
- [ ] **Extension Discovery**: Auto-discovery via `component_core::Extension` works
- [ ] **Config Loading**: SpinConfig integration loads from Spin variables
- [ ] **Context Propagation**: TraceContext propagates correctly
- [ ] **Formatter Integration**: All formatters produce expected output
- [ ] **Transport Integration**: WasmStdoutAdapter transports correctly

### 🎯 **End-to-End Tests with real_world_verification**
- [ ] **Agent Lifecycle**: Full agent startup → logging → shutdown cycle
- [ ] **Error Scenarios**: Graceful handling of config errors, transport failures
- [ ] **Performance**: No significant overhead in logging hot paths
- [ ] **Memory Safety**: No leaks under normal and error conditions  
- [ ] **WASM Compatibility**: Runs successfully in SpinKube environment
- [ ] **Feature Flag Testing**: All feature combinations work correctly

### 📊 **Performance Benchmarks**
- [ ] **Logging Throughput**: >5K structured log entries/second in WASM
- [ ] **Memory Usage**: <1MB total memory footprint for default configuration
- [ ] **Startup Time**: <25ms additional startup time for initialization
- [ ] **Context Overhead**: <20ns overhead for context-enriched logging

### 🔍 **Observability Validation**
- [ ] **Structured Output**: All log entries contain required structured fields
- [ ] **Trace Correlation**: trace_id propagated correctly across operations
- [ ] **Basic Metrics**: MetricsPort interface works with simple metrics
- [ ] **Error Tracking**: All error conditions logged with appropriate context
- [ ] **Health Monitoring**: Extension health checks report accurate status

### 🚀 **Production Readiness**
- [ ] **Graceful Degradation**: Continues working if transports fail
- [ ] **Resource Limits**: Respects memory constraints in WASM
- [ ] **Error Recovery**: Recovers from transient formatting errors
- [ ] **Multi-Agent Support**: Works correctly with multiple agent instances

## 📚 **Examples**

### Available Examples
- **`basic_metrics_demo.rs`**: Simple metrics collection
- **`integrated_context_demo.rs`**: Context management with dependency injection
- **`raii_scoped_context_demo.rs`**: Exception-safe context cleanup patterns
- **`standard_logging_demo.rs`**: Standard Rust logging integration
- **`tracing_and_kv_demo.rs`**: Tracing integration and structured fields

### Running Examples
```bash
# Standard logging integration
cargo run --example standard_logging_demo --features structured-logging

# Metrics demonstration
cargo run --example basic_metrics_demo --features structured-logging

# Context management  
cargo run --example raii_scoped_context_demo

# All features
cargo run --example integrated_context_demo
```

## 🛠️ **Development & Testing**

### Run Tests
```bash
# Unit tests
cargo test

# Integration tests  
cargo test --features structured-logging

# All features
cargo test --features observability
```

### Verify Extension Registration
```bash
# Check extension auto-discovery
cargo check --features structured-logging
```

## 🎯 **Usage Patterns**

### Basic Agent Integration
```rust
// Automatic initialization via AgentBuilder
let agent = AgentBuilder::from_config("config.json")?.init()?;

// Standard logging works immediately  
log::info!("Agent ready for requests");
```

### Custom Processing
```rust
// Add custom log processors
let chain = ProcessorChain::new()
    .add_processor(Box::new(TimestampProcessor))
    .add_processor(Box::new(ContextEnrichmentProcessor))
    .add_processor(Box::new(StructuredFieldsProcessor));
```

### Metrics Usage
```rust
// Use MetricsPort for basic metrics
let metrics_entry = MetricsEntry {
    name: "request_count".to_string(),
    metric_type: BasicMetricType::Counter,
    value: 1.0,
    labels: HashMap::new(),
    timestamp: Utc::now(),
};
```

---

**Foundation** • **WASM-Native** • **Zero Dependencies** • **Hexagonal Architecture** 