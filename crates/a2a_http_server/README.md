# A2A HTTP Server - Logging Strategy

## 🎯 Overview

The A2A HTTP Server implements comprehensive logging throughout critical operation paths to enable observability and debugging without external dependencies. This foundation is designed to integrate with the PromptFleet **`observability`** facade (happy path) for structured logs, traces, and metrics.

## 📋 Logging Architecture

### **Core Principles**
- **Agent-Centric**: All logs include agent ID for multi-agent environments
- **Target-Aware**: Distinguishes between WASM and Native implementations
- **Request Lifecycle**: Complete request tracing from entry to response
- **Error Context**: Rich error information with recovery hints
- **Performance Monitoring**: Size tracking and timing markers

### **Log Levels Strategy**

| Level | Usage | Examples |
|-------|--------|----------|
| **ERROR** | Protocol failures, system errors, request rejection | Failed JSON parsing, A2A protocol errors, serialization failures |
| **WARN** | Malformed requests, deprecated features, fallbacks | Invalid HTTP methods, unknown paths, configuration warnings |
| **INFO** | Request completion, agent lifecycle events | Agent creation, server startup, successful request completion |
| **DEBUG** | Request entry/exit, protocol delegation, validation steps | Request routing, protocol method calls, response generation |
| **TRACE** | Detailed payload inspection (development only) | Request/response bodies, headers, detailed execution flow |

## 🔍 Critical Logging Points

### **1. Request Lifecycle Logging**

```rust
// Entry Point - All Requests
debug!("Incoming request: {} {} for agent: {}", method, path, agent_id);
trace!("Request headers: {:?}", req.headers());

// Exit Point - All Requests  
info!("Request completed: {} {} -> {} for agent: {}", method, path, status, agent_id);
error!("Request failed: {} {} -> {} for agent: {}", method, path, error, agent_id);
```

### **2. JSON-RPC Processing**

```rust
// JSON-RPC Request Parsing
debug!("Parsing JSON-RPC request for agent: {} (size: {} bytes)", agent_id, size);
debug!("Successfully parsed JSON-RPC: method={} id={:?} for agent: {}", method, id, agent_id);
error!("Failed to parse JSON-RPC request for agent: {} - {}", agent_id, error);

// Protocol Delegation
debug!("Delegating to A2A protocol instance for agent: {} method: {}", agent_id, method);
debug!("A2A protocol returned response for agent: {} id: {:?}", agent_id, response_id);
error!("A2A protocol error for agent: {} - {}", agent_id, error);
```

### **3. Agent Creation & Initialization**

```rust
// Server Creation
debug!("Creating A2A HTTP server with agent_id: {}", agent_id);
info!("Registered A2A standard methods for agent: {}", agent_id);

// Target-Specific Information
info!("Created A2A HTTP Server (WASM/Spin) for agent: {}", agent_id);
info!("Created A2A HTTP Server (Native/Axum) for agent: {}", agent_id);
```

### **4. Response Generation**

```rust
// Response Serialization
debug!("Serialized response for agent: {} (size: {} bytes)", agent_id, size);
trace!("Response body: {}", response_body);
error!("Failed to serialize response for agent: {} - {}", agent_id, error);

// HTTP Response
debug!("Returning HTTP 200 response for agent: {}", agent_id);
```

### **5. Health & Discovery Endpoints**

```rust
// Health Checks
debug!("Serving health check for agent: {}", agent_id);
debug!("Health check completed for agent: {}", agent_id);

// Agent Card Discovery
debug!("Serving agent card for agent: {}", agent_id);
info!("Agent card served for agent: {}", agent_id);
```

## 🏗️ Implementation Details

### **WASM Server (Spin SDK)**
- **Entry Point**: `serve_request()` - Main Spin HTTP handler
- **Routing**: Path-based routing with comprehensive logging
- **Context**: Agent ID extracted and included in all log messages
- **Performance**: Request/response size tracking

### **Native Server (Axum)**
- **Entry Point**: `serve_request()` - Test compatibility simulation
- **Axum Handlers**: Separate logging for production Axum routes
- **Context**: Consistent agent ID context across all handlers
- **Testing**: Enhanced logging for integration test scenarios

### **Unified Patterns**

Both implementations follow identical logging patterns:

```rust
// Pattern: Agent-Context Logging
let agent_id = self.agent_id();
debug!("Operation for agent: {}", agent_id);

// Pattern: Size-Aware Logging  
debug!("Processing data for agent: {} (size: {} bytes)", agent_id, data.len());

// Pattern: Method Extraction from JsonRpcIncoming
let (method, id) = match &incoming {
    JsonRpcIncoming::Request(req) => (req.method.clone(), Some(&req.id)),
    JsonRpcIncoming::Notification(notif) => (notif.method.clone(), None),
    #[cfg(feature = "batch")]
    JsonRpcIncoming::Batch(_) => ("batch".to_string(), None),
};
```

## 🔧 Integration Points

### **Observability Facade Integration**

The logging foundation is designed for seamless integration with the `observability` facade:

```rust
// Initialize global observability once (recommended happy path)
use observability::{Obs, ObservabilityConfig};

// In main application
let _obs = Obs::init(ObservabilityConfig::default().with_service("my-agent", "0.1.0", "default"))?;
let server = A2AHttpServer::new_with_a2a_methods(agent_card);

// All server logs will flow through the global logger configured by `Obs::init(...)`
```

### **Observability Context**

When integrated with the observability system:

```rust
// Context will be automatically applied
log::info!("Request processed"); 
// Becomes: {"level":"INFO","message":"Request processed","trace_id":"abc","agent_id":"my-agent"}
```

## 📊 Log Output Examples

### **Successful Request Flow**
```
DEBUG [my-agent] Incoming request: POST /jsonrpc for agent: my-agent
DEBUG [my-agent] Parsing JSON-RPC request for agent: my-agent (size: 156 bytes)  
DEBUG [my-agent] Successfully parsed JSON-RPC: method=agent/ping id="req-123" for agent: my-agent
DEBUG [my-agent] Delegating to A2A protocol instance for agent: my-agent method: agent/ping
DEBUG [my-agent] A2A protocol returned response for agent: my-agent id: "req-123"
DEBUG [my-agent] Serialized response for agent: my-agent (size: 89 bytes)
DEBUG [my-agent] Returning HTTP 200 response for agent: my-agent
INFO  [my-agent] Request completed: POST /jsonrpc -> 200 for agent: my-agent
```

### **Error Scenario**
```
DEBUG [my-agent] Incoming request: POST /jsonrpc for agent: my-agent
DEBUG [my-agent] Parsing JSON-RPC request for agent: my-agent (size: 64 bytes)
ERROR [my-agent] Failed to parse JSON-RPC request for agent: my-agent - expected value at line 1 column 1
ERROR [my-agent] Request failed: POST /jsonrpc -> invalid JSON for agent: my-agent
```

### **Agent Discovery**
```
DEBUG [my-agent] Creating A2A HTTP server with standard methods for agent: my-agent
DEBUG [my-agent] Initialized in-memory task storage for agent: my-agent
INFO  [my-agent] Registered A2A standard methods for agent: my-agent
INFO  [my-agent] Created A2A HTTP Server (WASM/Spin) for agent: my-agent
```

## 🚀 Usage Recommendations

### **Development**
```rust
// Enable debug logging for development
RUST_LOG=debug cargo run

// Or use the convenience function
a2a_http_server::init_logging("my-agent");
```

### **Production**
```rust
// JSON logging for production containers (configured via ObservabilityConfig.logging.format)
RUST_LOG=info cargo run

// With the observability facade
use observability::{Obs, ObservabilityConfig};
let _obs = Obs::init(ObservabilityConfig::default().with_service("my-agent", "0.1.0", "default"))?;
```

### **Testing**
```rust
// Capture logs in tests
use log::LevelFilter;
env_logger::Builder::from_default_env()
    .filter_level(LevelFilter::Debug)
    .init();
```

## 🔮 Future Enhancements

The current logging foundation enables:

1. **Metrics Integration**: Request duration, throughput, error rates
2. **Tracing Integration**: Distributed tracing across agent networks  
3. **Structured Fields**: Rich context beyond agent ID
4. **Performance Analytics**: Hot path optimization based on logs
5. **Security Monitoring**: Request pattern analysis

This logging strategy provides a solid foundation for comprehensive observability while maintaining clean architecture principles and preparing for future integration with the full observability stack.
