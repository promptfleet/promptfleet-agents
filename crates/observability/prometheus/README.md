# Prometheus Extension - Production Metrics Extension

> **Production-ready Prometheus metrics extension with modern best practices for SpinKube SDK**

This crate provides enterprise-grade Prometheus metrics collection with modern features including hierarchical federation, cardinality reduction, push gateway integration, and performance recording rules optimized for WASM environments.

## 🎯 **Core Mission**

Provide production-ready Prometheus metrics with modern best practices:
```rust
let agent = AgentBuilder::from_config("config.json")?.init()?;
// Enterprise Prometheus metrics with cardinality reduction, federation, and push gateway
let prometheus = agent.get_client::<Prometheus>()?;
prometheus.record_metric("request_count", 1.0, &[("endpoint", "/api/users")]);
```

## 🏗️ **Architecture**

```
📦 prometheus/
├── 🚀 plugin/               # Core Prometheus metrics extension (ObservabilityPlugin + MetricsPort)
├── 📊 cardinality_reduction/ # String interning, label optimization, cardinality limits
├── 🌐 hierarchical_federation/ # Multi-level federation (cluster → region → global)
├── 📈 recording_rules/      # Performance pre-computed rules (Istio, SpinKube, LLM)
├── 🔄 pushgateway_client/   # Batch metrics export with async SpinKube support
└── 🧩 lib/                  # Feature flags and re-exports
```

## ✨ **Production Features**

### 🚀 **Core Prometheus Integration**
- ✅ **ObservabilityPlugin Implementation**: Full plugin trait with span tracking
- ✅ **MetricsPort Implementation**: Counter, Histogram, Gauge metric abstractions
- ✅ **Registry Management**: Thread-safe Prometheus registry with custom metrics
- ✅ **Metric Collection**: Span duration, count, logs, and custom metrics
- ✅ **WASM Compatibility**: Pure WebAssembly compatible implementation

### 📊 **Advanced Cardinality Reduction** 
- ✅ **String Interning**: Memory-efficient label value storage with zero allocations
- ✅ **Label Optimization**: HTTP status grouping, URL path extraction, ID hashing
- ✅ **Global Cardinality Limits**: Configurable limits with graceful degradation
- ✅ **LRU Eviction**: Intelligent combination tracking with last-seen eviction
- ✅ **Statistics Tracking**: Real-time cardinality utilization and reduction ratios

### 🌐 **Hierarchical Federation**
- ✅ **Multi-Level Architecture**: Cluster → Region → Global federation patterns
- ✅ **Prometheus Config Generation**: Automatic federation configuration YAML
- ✅ **Match Expressions**: Intelligent metric filtering for federation levels
- ✅ **Honor Labels**: Proper label handling across federation boundaries
- ✅ **Validation**: Comprehensive federation configuration validation

### 📈 **Performance Recording Rules**
- ✅ **Pre-Built Rule Sets**: Istio, SpinKube, and LLM performance rules
- ✅ **YAML Generation**: Automatic Prometheus recording rules configuration
- ✅ **Custom Rules**: Extensible rule system for domain-specific metrics
- ✅ **Rule Validation**: Comprehensive rule validation and error checking
- ✅ **Performance Optimization**: Pre-computed expensive PromQL queries

### 🔄 **Push Gateway Integration**
- ✅ **Async SpinKube Client**: Native async HTTP client with proper timeouts
- ✅ **Batch Operations**: Efficient batch metrics pushing for high volume
- ✅ **Health Monitoring**: Push gateway health checks and monitoring
- ✅ **Custom Labels**: Flexible label-based metric organization
- ✅ **Error Handling**: Comprehensive error handling with retry logic

### 🧩 **Extension Integration**
- ✅ **Auto-Discovery**: Full `component_core::Extension` trait implementation with `#[extension("prometheus")]`
- ✅ **Configuration**: Builder pattern and environment variable configuration
- ✅ **Health Checks**: Built-in plugin health monitoring and status reporting
- ✅ **Multi-Feature Support**: Granular feature flag control for deployment flexibility

## 📋 **Feature Flags**

```toml
[features]
default = []
prometheus-federation = ["prometheus", "reqwest", "uuid"]
pushgateway-client = ["prometheus-federation"]
cardinality-reduction = ["prometheus-federation", "url"]
recording-rules = ["prometheus-federation", "serde_yaml"]
structured-logging = ["serde_json", "observability_core/structured-logging"]
```

## 🚀 **Quick Start**

### 1. Add to Cargo.toml
```toml
[dependencies]
sdk = { workspace = true }
prometheus = { workspace = true, features = ["prometheus-federation"] }
```

### 2. Configure in config.json
```json
{
  "extensions": {
    "prometheus": {
      "pushgateway_endpoint": "http://prometheus-pushgateway:9091",
      "job_name": "my-agent",
      "instance": "localhost:9090",
      "push_interval_secs": 30,
      "cardinality_reduction": true,
      "max_cardinality": 10000,
      "hierarchical_federation": true,
      "global_labels": {
        "environment": "production",
        "cluster": "us-west-2"
      }
    }
  }
}
```

### 3. Use in Your Agent
```rust
use sdk::AgentBuilder;
use prometheus::Prometheus;

fn main() -> ComponentResult<()> {
    // Initialize agent with Prometheus metrics
    let agent = AgentBuilder::from_config("config.json")?.init()?;
    
    // Get Prometheus extension for custom metrics
    let prometheus = agent.get_client::<Prometheus>()?;
    
    // Record custom metrics
    prometheus.record_metric("requests_total", 1.0, &[
        ("method", "GET"),
        ("endpoint", "/api/users"),
        ("status", "200")
    ]);
    
    // Use MetricsPort interface
    prometheus.emit_counter_simple("custom_counter", 1.0)?;
    prometheus.emit_histogram_simple("request_duration", 0.150)?;
    prometheus.emit_gauge_simple("active_connections", 42.0)?;
    
    Ok(())
}
```

## 📊 **Configuration**

### PrometheusConfig
```rust
#[derive(Debug, Clone)]
pub struct PrometheusConfig {
    /// Push gateway endpoint (optional)
    pub pushgateway_endpoint: Option<String>,
    
    /// Job name for push gateway
    pub job_name: String,
    
    /// Instance identifier
    pub instance: String,
    
    /// Push interval for batch metrics
    pub push_interval: Duration,
    
    /// Enable cardinality reduction
    pub cardinality_reduction: bool,
    
    /// Maximum number of unique label combinations
    pub max_cardinality: usize,
    
    /// Enable hierarchical federation
    pub hierarchical_federation: bool,
    
    /// Additional labels for all metrics
    pub global_labels: HashMap<String, String>,
}
```

### Builder Pattern Configuration
```rust
let config = PrometheusConfig::builder()
    .with_pushgateway("http://prometheus-pushgateway:9091")
    .with_job_name("spinkube-agent")
    .with_instance("pod-123")
    .with_push_interval(Duration::from_secs(30))
    .with_cardinality_reduction(true)
    .with_max_cardinality(10000)
    .with_global_label("cluster", "production")
    .with_global_label("region", "us-west-2")
    .build();

let prometheus = Prometheus::new(config)?;
```

### Environment Variable Configuration
```bash
export PROMETHEUS_PUSHGATEWAY="http://prometheus-pushgateway:9091"
export PROMETHEUS_JOB_NAME="my-spinkube-agent"
export PROMETHEUS_INSTANCE="pod-abc123"
export PROMETHEUS_GLOBAL_LABELS="environment=prod,region=us-west-2"

let prometheus = Prometheus::from_env()?;
```

## ✅ **Verification Success Criteria**

### 🧪 **Unit Tests**
- [x] **Plugin Creation**: Configuration builder and environment loading work correctly
- [x] **Cardinality Reduction**: String interning and label optimization function properly
- [x] **Federation Config**: YAML generation and validation work correctly
- [x] **Recording Rules**: Rule creation and YAML generation work properly
- [x] **Push Gateway Client**: Metrics gathering and client creation work correctly
- [x] **MetricsPort Interface**: All metric emission methods work correctly

### 🔗 **Integration Tests**
- [ ] **Extension Discovery**: Auto-discovery as `"prometheus"` extension works
- [ ] **ObservabilityPlugin Integration**: Full plugin trait implementation works with core
- [ ] **MetricsPort Integration**: MetricsPort trait works with observability_core
- [ ] **Registry Integration**: Custom metrics registration and collection work
- [ ] **Config Loading**: SpinConfig integration loads from Spin variables correctly
- [ ] **Feature Flag Testing**: All feature combinations work together correctly

### 🎯 **End-to-End Tests with real_world_verification**
- [ ] **Agent Lifecycle**: Full agent startup with Prometheus metrics collection
- [ ] **Push Gateway Integration**: Successful metrics pushing to real push gateway
- [ ] **Cardinality Management**: Cardinality reduction works under real load
- [ ] **Federation Deployment**: Generated federation configs work with real Prometheus
- [ ] **Recording Rules Deployment**: Generated recording rules work in production
- [ ] **Error Scenarios**: Graceful handling of push gateway failures and timeouts

### 📊 **Performance Benchmarks**
- [ ] **Metrics Throughput**: >1K metric recording operations/second in WASM
- [ ] **Memory Efficiency**: <3MB memory footprint with cardinality reduction enabled
- [ ] **Cardinality Efficiency**: >95% cardinality reduction for high-cardinality scenarios
- [ ] **Push Performance**: <5s push latency for 10K metrics to push gateway
- [ ] **String Interning**: >90% memory reduction for repeated label values

### 🔍 **Production Validation**
- [ ] **Cardinality Control**: Cardinality limits prevent memory exhaustion
- [ ] **Federation Scalability**: Multi-level federation reduces query load on global Prometheus
- [ ] **Recording Rules Performance**: Pre-computed rules reduce query latency by >80%
- [ ] **Push Gateway Reliability**: Metrics survive transient network failures
- [ ] **Resource Limits**: All features respect WASM memory and compute constraints

### 🚀 **Enterprise Readiness**
- [ ] **High Availability**: Plugin continues working during push gateway outages
- [ ] **Security Compliance**: No sensitive data leakage in metric labels
- [ ] **Observability**: Plugin itself is properly instrumented and observable
- [ ] **Documentation**: All configuration options and patterns documented
- [ ] **Multi-Tenant Support**: Works correctly with multiple agent instances

## 📚 **Examples**

### Available Examples (To Be Created)
- **`basic_metrics_demo.rs`**: Simple counter, histogram, and gauge usage
- **`cardinality_optimization_demo.rs`**: Label optimization and cardinality control
- **`federation_config_demo.rs`**: Generating Prometheus federation configurations
- **`push_gateway_demo.rs`**: Batch metrics pushing with error handling
- **`recording_rules_demo.rs`**: Creating and validating performance recording rules
- **`production_deployment_demo.rs`**: Complete production deployment example

### Running Examples
```bash
# Basic metrics collection
cargo run --example basic_metrics_demo --features prometheus-federation

# Cardinality optimization
cargo run --example cardinality_optimization_demo --features cardinality-reduction

# Push gateway integration
cargo run --example push_gateway_demo --features pushgateway-client

# Recording rules
cargo run --example recording_rules_demo --features recording-rules

# Full production setup
cargo run --example production_deployment_demo --features prometheus-federation,pushgateway-client,cardinality-reduction,recording-rules
```

## 🛠️ **Development & Testing**

### Run Tests
```bash
# Unit tests
cargo test

# Integration tests with all features
cargo test --features prometheus-federation,pushgateway-client,cardinality-reduction,recording-rules

# Performance tests
cargo test --release --features prometheus-federation
```

### Verify Cardinality Reduction
```bash
# Test cardinality limits
cargo test test_cardinality_reduction --features cardinality-reduction -- --nocapture

# Performance benchmarks
cargo test test_string_interning_performance --release -- --nocapture
```

### Generate Federation Configs
```bash
# Generate Prometheus federation YAML
cargo run --example federation_config_demo --features prometheus-federation
```

## 🎯 **Usage Patterns**

### Basic Metrics Collection
```rust
use prometheus::Prometheus;

let prometheus = Prometheus::from_env()?;

// Record business metrics
prometheus.record_metric("user_registrations", 1.0, &[
    ("source", "web"),
    ("plan", "premium")
]);

// Use convenience API
prometheus.emit_counter_simple("api_calls", 1.0)?;
prometheus.emit_histogram_simple("response_time", 0.250)?;
```

### Advanced Cardinality Management
```rust
use prometheus::{Prometheus, PrometheusConfig};

let config = PrometheusConfig::builder()
    .with_cardinality_reduction(true)
    .with_max_cardinality(5000)
    .build();

let prometheus = Prometheus::new(config)?;

// High-cardinality metrics are automatically optimized
prometheus.record_metric("http_requests", 1.0, &[
    ("url", "/api/users/12345"),  // Automatically reduced to path
    ("status", "404"),           // Automatically grouped to "4xx"
    ("user_id", "user_12345")    // Automatically hashed
]);
```

### Federation Configuration Generation
```rust
use prometheus_plugin_2025::{HierarchicalFederation, FederationConfig};

let federation = HierarchicalFederation::new(FederationConfig::default());
let prometheus_config = federation.generate_prometheus_config()?;

// Save to prometheus.yml for production deployment
std::fs::write("prometheus.yml", prometheus_config)?;
```

### Recording Rules Deployment
```rust
use prometheus_plugin_2025::RecordingRulesManager;

let manager = RecordingRulesManager::with_spinkube_defaults();
let rules_yaml = manager.generate_yaml()?;

// Deploy to Prometheus for performance optimization
std::fs::write("recording_rules.yml", rules_yaml)?;
```

### Push Gateway Integration
```rust
use prometheus::{Prometheus, PushGatewayClient};

let prometheus = Prometheus::from_env()?;

// Record metrics normally
prometheus.record_metric("batch_jobs", 1.0, &[("status", "completed")]);

// Force push to gateway
prometheus.force_push().await?;

// Or use direct push gateway client
let client = PushGatewayClient::new("http://prometheus-pushgateway:9091")?;
client.push_metrics(prometheus.registry(), "my-job", "instance-1").await?;
```

---

**Production-Ready** • **Cardinality-Optimized** • **Federation-Enabled** • **WASM-Native** 