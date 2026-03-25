# a2a_http_server

A2A Protocol v1.0 HTTP server. Targets WASM (Fermyon Spin SDK) and native (Axum) with an identical API surface.

## Quick start

### Standalone (protocol-only, no application layer)

```rust
use a2a_http_server::A2AHttpServer;
use a2a_protocol_core::AgentCard;

let card = AgentCard::new("my-agent")
    .with_capability("SendMessage", "Process user messages");

let server = A2AHttpServer::new_with_a2a_methods(card);
```

### With application port (recommended for production)

```rust
use a2a_http_server::A2AHttpServer;
use a2a_app_ports::A2AAppPortAsync;
use std::sync::Arc;

let server = A2AHttpServer::with_app_port_async(Arc::new(MyAgent));
```

See [`a2a_app_ports`](../a2a_app_ports/README.md) for implementing `A2AAppPort` / `A2AAppPortAsync`.

## Endpoints

| Path | Method | Description |
|------|--------|-------------|
| `/jsonrpc` | `POST` | JSON-RPC 2.0 dispatch for all A2A methods |
| `/.well-known/agent-card.json` | `GET` | Agent card (A2A discovery) |
| `/v1/agent/card:get` | `GET` | Agent card (alternate path) |
| `/health` | `GET` | Liveness probe — returns `{"status":"healthy","agent":"<id>"}` |

## A2A method names

```rust
use a2a_http_server::method;

method::PING                     // "Ping"
method::SEND_MESSAGE             // "SendMessage"
method::SEND_STREAMING_MESSAGE   // "SendStreamingMessage"
method::GET_AGENT_CARD           // "GetAgentCard"
method::GET_EXTENDED_AGENT_CARD  // "GetExtendedAgentCard"
method::GET_TASK                 // "GetTask"
method::CANCEL_TASK              // "CancelTask"
method::LIST_TASKS               // "ListTasks"
```

## Feature flags

| Flag | Enables |
|------|---------|
| `core` | Standard A2A methods via `a2a_protocol_core/core` (default) |
| `event-stream` / `streaming` | `SendStreamingMessage` + SSE streaming support |
| `observability` | OTEL spans, W3C trace-context propagation |
| `discovery` | Extended agent card discovery |
| `files` | File upload/download support |
| `push` | Push notification configuration endpoints |
| `schema` | JSON schema generation via `schemars` |
| `all` | All of the above |

## Target dispatch

| Target | Backend |
|--------|---------|
| `wasm32` | Fermyon Spin SDK HTTP handler |
| native | `axum` 0.7 with `tower-http` CORS |

Both targets expose the same `A2AHttpServer` type and `serve_request` method.

## Logging

The server logs at `debug` level for request lifecycle and `info` for agent events. Wire in any `log`-compatible backend (env_logger, the `observability` facade, etc.):

```rust
a2a_http_server::init_logging("my-agent");
```

For structured JSON logs with trace context, use the `observability` feature and the `observability` crate's `Obs::init(...)`.
