# a2a_http_client

A2A Protocol v1.0 HTTP client. Targets WASM (Fermyon Spin SDK) and native (Reqwest) with an identical API surface.

## Quick start

```rust
use a2a_http_client::Client;
use serde_json::json;

// Connect to a remote agent
let client = Client::external("https://my-agent.example.com/jsonrpc")
    .with_header("Authorization".to_string(), "Bearer <token>".to_string());

// Call any A2A method by name
let result = client.call("SendMessage", json!({
    "message": {
        "role": "ROLE_USER",
        "parts": [{"text": "Hello agent"}],
        "messageId": "msg-1"
    }
})).await?;
```

## Activation-aware calls (KEDA scale-to-zero)

When an agent is scaled to zero by KEDA, the first request may hit a cold-start window. Use `call_with_activation` for automatic retry with exponential backoff:

```rust
use a2a_http_client::{Client, ActivationConfig};

let client = Client::external("https://my-agent.example.com/jsonrpc");
let config = ActivationConfig::default(); // up to 3 retries, 30 s timeout

let result = client.call_with_activation(
    "SendMessage",
    json!({ "message": { /* ... */ } }),
    &config,
    "req-idempotency-key",
).await?;
```

Retriable status codes: `502`, `503`, `504`, and connection-refused errors.

## Feature flags

| Flag | Enables |
|------|---------|
| `observability` | OTEL spans and W3C trace-context propagation via the `observability` facade |
| `streaming` | `futures-util` + `async-stream` + `a2a_protocol_core/event-stream` for SSE streaming |

## Target dispatch

The crate selects its HTTP backend at compile time:

| Target | Backend |
|--------|---------|
| `wasm32` | Fermyon Spin SDK outbound HTTP |
| native | `reqwest` with `rustls-tls` |

Both targets expose the same `Client` type via `pub use implementation::*`.

## `check_connectivity`

```rust
use a2a_http_client::check_connectivity;

// Returns Ok(()) if the endpoint responds, Err otherwise
check_connectivity("https://my-agent.example.com/jsonrpc").await?;
```
