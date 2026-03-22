# OpenTelemetry Extension - Enterprise Observability Extension

> **Enterprise-grade OpenTelemetry extension with OTLP export, auto-instrumentation, and smart sampling for PromptFleet Agent SDK**

This is an advanced observability extension providing comprehensive OpenTelemetry integration with OTLP collector export, automatic instrumentation, and intelligent sampling strategies built for WASM-native SpinKube environments.

## 🎯 **Core Mission**

Provide enterprise-grade OpenTelemetry capabilities with standards-compliant OTLP export:
```rust
let agent = AgentBuilder::from_config("config.json")?.init()?;
// Enterprise OpenTelemetry with OTLP export, auto-instrumentation, and smart sampling
let span = agent.get_client::<Otel>()?.start_span("llm_request");
```

## 🏗️ **Enterprise Architecture**

Built on `observability_core` foundation with enterprise-grade extensions:
```
📦 otel/
├── 🚀 plugin/              # Core extension implementation (Otel, configuration)
├── 📡 collector_client/    # OTLP/HTTP export to OpenTelemetry collectors  
├── 🎯 sampling/            # Smart sampling strategies (ratio, parent-based, always-on/off)
├── 🔧 resource_attributes/ # OpenTelemetry resource attribute management
├── ⚡ auto_instrumentation/ # Automatic HTTP and function instrumentation
└── 📊 batching/            # High-performance telemetry batching system
```

## ✨ **Enterprise Features**

### 📡 **OTLP Collector Export**
- ✅ **HTTP/JSON Transport**: Standards-compliant OTLP over HTTP
- ✅ **Batch Export**: High-performance batched telemetry export
- ✅ **Multiple Signal Types**: Spans, metrics, and logs export
- ✅ **Collector Health**: Health checking and collector capabilities discovery
- ✅ **Error Handling**: Robust error handling with retry strategies

### 🎯 **Smart Sampling Strategies**
- ✅ **Trace ID Ratio**: Deterministic sampling based on trace ID
- ✅ **Parent-Based**: Respect parent sampling decisions
- ✅ **Always On/Off**: Simple on/off sampling control
- ✅ **Custom Strategies**: Extensible sampling strategy framework
- ✅ **Performance Optimized**: Minimal overhead sampling decisions

### ⚡ **Auto-Instrumentation**
- ✅ **HTTP Client Instrumentation**: Automatic HTTP request tracing
- ✅ **Function Instrumentation**: Decorative function call tracing  
- ✅ **W3C Trace Propagation**: Standards-compliant trace context propagation
- ✅ **Request/Response Enrichment**: Automatic status code and metadata capture
- ✅ **Async/Sync Support**: Both async and synchronous instrumentation

### 🔧 **Resource Management**
- ✅ **Service Identity**: Service name, version, namespace management
- ✅ **Custom Attributes**: Flexible custom resource attribute support
- ✅ **Environment Integration**: OpenTelemetry standard environment variables
- ✅ **Resource Discovery**: Automatic resource attribute discovery
- ✅ **Attribute Validation**: Resource attribute validation and sanitization

### 📊 **High-Performance Export**
- ✅ **Memory-Efficient Batching**: Configurable batch sizes and memory limits
- ✅ **Async Export**: Non-blocking telemetry export
- ✅ **Compression Support**: Efficient payload compression (planned)
- ✅ **Export Statistics**: Real-time export performance monitoring
- ✅ **Backpressure Handling**: Graceful handling of collector unavailability

### 🌐 **Standards Compliance**
- ✅ **OpenTelemetry Specification**: Full compliance with OTel standards
- ✅ **W3C Trace Context**: W3C traceparent/tracestate support
- ✅ **OTLP Protocol**: OpenTelemetry Protocol over HTTP/JSON
- ✅ **Semantic Conventions**: OpenTelemetry semantic conventions support
- ✅ **Resource Semantics**: Standard resource semantic conventions

## 📋 **Feature Flags**

```toml
[features]
default = []
otel-2025 = ["opentelemetry", "opentelemetry-otlp", "opentelemetry-semantic-conventions"]
auto-instrumentation = ["otel-2025"]
grpc-tonic = ["opentelemetry-otlp/grpc-tonic"]
grpc-sys = ["opentelemetry-otlp/grpc-sys"]
structured-logging = ["serde_json", "observability_core/structured-logging"]
```

## 🚀 **Quick Start**

### 1. Add to Cargo.toml
```toml
[dependencies]
sdk = { workspace = true }
otel = { workspace = true, features = ["otel-2025", "auto-instrumentation"] }
```

### 2. Configure in config.json
```json
{
  "extensions": {
    "otel": {
      "enabled": true,
      "otlp_endpoint": "http://jaeger-collector:4318",
      "service_name": "my-agent",
      "service_version": "1.0.0",
      "service_namespace": "production",
      "batch_size": 512,
      "export_timeout": "30s",
      "sampling_strategy": {
        "type": "trace_id_ratio",
        "ratio": 0.1
      },
      "auto_instrumentation": true
    }
  }
}
```

### 3. Use Enterprise Features
```rust
use sdk::AgentBuilder;
use otel::{Otel, auto_instrument_async};

#[tokio::main]
async fn main() -> ComponentResult<()> {
    // Initialize agent with enterprise OpenTelemetry
    let agent = AgentBuilder::from_config("config.json")?.init()?;
    let otel = agent.get_client::<Otel>()?;
    
    // Manual instrumentation
    let span = otel.start_span("llm_request", &[
        ("model", "gpt-4"),
        ("operation", "chat_completion")
    ]);
    
    // Automatic instrumentation with macro
    let result = auto_instrument_async!(
        Arc::new(otel.clone()),
        "complex_operation",
        &[("component", "business_logic")],
        async {
            // Your business logic here
            process_llm_request().await
        }
    );
    
    span.add_attribute("tokens", "1500");
    span.set_status(SpanStatus::Ok);
    
    Ok(())
}
```

## 📊 **Configuration**

### OtelConfig
```rust
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP collector endpoint (e.g., "http://jaeger:4318")
    pub otlp_endpoint: String,
    
    /// Service identification
    pub service_name: String,
    pub service_version: String,
    pub service_namespace: String,
    
    /// Export configuration
    pub batch_size: usize,              // Default: 512
    pub export_timeout: Duration,       // Default: 30s
    
    /// Sampling strategy
    pub sampling_strategy: SamplingStrategy,
    
    /// Feature flags
    pub auto_instrumentation: bool,
    
    /// Custom resource attributes
    pub resource_attributes: HashMap<String, String>,
}
```

### Sampling Strategies
```rust
// Trace ID ratio sampling (10% of traces)
let sampling = SamplingStrategy::trace_id_ratio(0.1);

// Parent-based sampling with custom root strategy
let sampling = SamplingStrategy::parent_based();

// Always sample (for development)
let sampling = SamplingStrategy::always_on();

// Never sample (to disable tracing)
let sampling = SamplingStrategy::always_off();
```

### Environment Variables (OpenTelemetry Standard)
```bash
# Collector endpoint
export OTEL_EXPORTER_OTLP_ENDPOINT="http://collector:4318"

# Service identification
export OTEL_SERVICE_NAME="my-spinkube-agent"
export OTEL_SERVICE_VERSION="1.2.3"
export OTEL_SERVICE_NAMESPACE="production"

# Resource attributes
export OTEL_RESOURCE_ATTRIBUTES="environment=prod,region=us-west-2"
```

## ✅ **Verification Success Criteria**

### 🧪 **Unit Tests**
- [x] **Plugin Configuration**: All config parsing and builder patterns work
- [x] **Sampling Strategies**: All sampling algorithms work correctly
- [x] **Resource Attributes**: Resource attribute management functions properly
- [x] **OTLP Formatting**: OTLP payload generation matches specification
- [x] **Auto-Instrumentation**: HTTP and function instrumentation works
- [x] **Span Management**: Span lifecycle and attribute management

### 🔗 **Integration Tests**  
- [ ] **OTLP Export**: End-to-end export to real OTLP collectors (Jaeger, Zipkin)
- [ ] **Extension Discovery**: Auto-discovery as `"otel"` extension works
- [ ] **Trace Propagation**: W3C trace context propagates across services
- [ ] **Sampling Integration**: Sampling strategies work in distributed scenarios
- [ ] **Auto-Instrumentation**: HTTP client instrumentation works in real scenarios
- [ ] **Resource Discovery**: Environment-based resource attribute discovery

### 🎯 **End-to-End Tests with real_world_verification**
- [ ] **Agent-to-Collector**: Full agent → OTLP collector → observability backend
- [ ] **Multi-Agent Tracing**: Distributed tracing across multiple agents
- [ ] **Performance Under Load**: High-throughput export without data loss
- [ ] **Collector Failover**: Graceful handling of collector unavailability
- [ ] **Memory Efficiency**: Bounded memory usage under sustained load
- [ ] **SpinKube Integration**: Works correctly in SpinKube environment

### 📊 **Performance Benchmarks**
- [ ] **Export Throughput**: >10K spans/second export to collector
- [ ] **Memory Efficiency**: <5MB memory footprint for default configuration
- [ ] **Sampling Overhead**: <100ns overhead for sampling decisions
- [ ] **Instrumentation Overhead**: <500ns overhead for auto-instrumentation
- [ ] **Batch Export Efficiency**: >95% successful export rate under normal load

### 🔍 **Enterprise Validation**
- [ ] **OTLP Compliance**: Full OpenTelemetry Protocol specification compliance
- [ ] **Standards Conformance**: W3C trace context and OTel semantic conventions
- [ ] **Collector Compatibility**: Works with Jaeger, Zipkin, Grafana Tempo, AWS X-Ray
- [ ] **Security**: Secure transport and authentication support
- [ ] **Observability**: Self-monitoring and health checking capabilities

### 🚀 **Production Readiness**
- [ ] **High Availability**: Continues working when collector is unavailable
- [ ] **Resource Limits**: Respects memory and CPU constraints in WASM
- [ ] **Error Recovery**: Recovers from transient network and collector errors
- [ ] **Configuration Validation**: Validates configuration and provides helpful errors
- [ ] **Multi-Tenant Support**: Works correctly with multiple agent instances

## 📚 **Examples**

### Available Examples
- **`basic_otlp_export.rs`**: Simple OTLP export to collector
- **`auto_instrumentation_demo.rs`**: HTTP and function auto-instrumentation
- **`sampling_strategies_demo.rs`**: Different sampling strategy configurations
- **`enterprise_config_demo.rs`**: Enterprise configuration patterns
- **`distributed_tracing_demo.rs`**: Multi-service distributed tracing
- **`performance_benchmark.rs`**: Performance and throughput testing

### Running Examples
```bash
# Basic OTLP export
cargo run --example basic_otlp_export --features otel-2025

# Auto-instrumentation demo
cargo run --example auto_instrumentation_demo --features auto-instrumentation

# Sampling strategies
cargo run --example sampling_strategies_demo --features otel-2025

# Enterprise configuration
cargo run --example enterprise_config_demo --features otel-2025,structured-logging

# Performance testing
cargo run --example performance_benchmark --features otel-2025 --release
```

## 🛠️ **Development & Testing**

### Run Tests
```bash
# Unit tests
cargo test

# Integration tests with collector
cargo test --features otel-2025 -- --ignored

# All features
cargo test --features otel-2025,auto-instrumentation,structured-logging

# Performance tests
cargo test --release --features otel-2025 performance
```

### Local Collector Setup
```bash
# Start Jaeger for testing
docker run -d --name jaeger \
  -p 16686:16686 \
  -p 4317:4317 \
  -p 4318:4318 \
  jaegertracing/all-in-one:latest

# Run integration tests
cargo test --features otel-2025 integration_tests
```

## 🎯 **Usage Patterns**

### Enterprise Agent Setup
```rust
// Environment-based configuration
let otel = Otel::from_env().await?;

// Or builder pattern
let otel = Otel::builder()
    .with_endpoint("http://tempo:4318")
    .with_service_name("my-llm-agent")
    .with_batch_size(1024)
    .build()
    .await?;
```

### Auto-Instrumentation
```rust
use otel::{auto_instrument, AutoInstrumentedHttpClient};

// Instrumented HTTP client
let client = AutoInstrumentedHttpClient::new(
    reqwest::Client::new(),
    Arc::new(otel)
);

let response = client.get("https://api.openai.com/v1/models").await?;
```

### Custom Sampling
```rust
let config = OtelConfig::builder()
    .with_sampling_strategy(
        SamplingStrategy::parent_based()
    )
    .build();
```

### Resource Attributes
```rust
let config = OtelConfig::builder()
    .with_resource_attribute("deployment.environment", "production")
    .with_resource_attribute("kubernetes.cluster.name", "spinkube-prod")
    .with_resource_attribute("kubernetes.namespace.name", "agents")
    .build();
```

---

**Enterprise** • **OTLP Export** • **Auto-Instrumentation** • **Standards Compliant** • **High Performance** 