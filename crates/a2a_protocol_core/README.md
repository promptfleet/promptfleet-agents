# a2a_protocol_core

Pure A2A (Agent-to-Agent) Protocol v1.0 domain logic. Transport-agnostic, WASM-first, no HTTP dependencies.

## Role in the stack

```
a2a_http_server  ──► a2a_protocol_core  ──► protocol_transport_core (JSON-RPC 2.0)
a2a_http_client  ──► a2a_protocol_core
agent_sdk        ──► a2a_protocol_core
```

This crate owns the protocol state machine, method registry, data types, and task storage. It never touches sockets or HTTP.

## Quick start

```rust
use a2a_protocol_core::{A2AProtocol, AgentCard, methods::messaging::handle_message_send};
use a2a_protocol_core::services::InMemoryTaskStorage;
use std::sync::Arc;

let card = AgentCard::new("my-agent")
    .with_capability("SendMessage", "Process user messages");

let storage = Arc::new(InMemoryTaskStorage::new());
let mut protocol = A2AProtocol::new(card, storage);

// Register method handlers
protocol.register_method("SendMessage", Arc::new(|req, storage| {
    handle_message_send(req, storage).map_err(Into::into)
}));

// Dispatch a JSON-RPC request
let request = serde_json::from_str(raw_bytes)?;
let response = protocol.handle_request(request)?;
```

## Feature flags

| Flag | Enables | Default |
|------|---------|---------|
| `protocol-core` | `Task`, `Message`, `Artifact`, `AgentCard`, `TaskStorage` | yes |
| `event-stream` | `StreamResponse`, `SendStreamingMessage` handler | no |
| `file-handling` | URL and raw-bytes `Part` variants in `should_create_task` | no |
| `time-stamps` | `chrono`-based timestamps on discovery responses | no |
| `all-features` | All of the above | no |
| `spec-compliant` | `protocol-core` + `event-stream` (A2A spec minimum) | no |
| `wasm-optimized` | `protocol-core` only (smallest WASM footprint) | no |

## Module overview

| Module | Contents |
|--------|----------|
| `agent` | `AgentCard`, `AgentCapabilities`, `AgentSkill`, builder methods |
| `protocol` | `A2AProtocol` — method registry + request dispatcher |
| `registry` | `A2AMethodRegistry`, `A2AMethodHandler`, `MethodMetadata` |
| `error` | `A2AError`, `A2AResult`, `a2a_error_codes` constants |
| `transport` | `A2ATransport` trait, `A2ATransportFactory` |
| `security` | `SecurityScheme` union (ApiKey, HTTP, OAuth2, OIDC, mTLS) |
| `data::task` | `Task`, `TaskStatus`, `TaskState` |
| `data::message` | `Message`, `MessageRole`, `Part` |
| `data::artifact` | `Artifact` |
| `data::notification` | `TaskPushNotificationConfig`, `AuthenticationInfo` |
| `methods::messaging` | `handle_message_send`, `handle_tasks_send_subscribe` |
| `methods::tasks` | `handle_tasks_get`, `handle_tasks_cancel`, `handle_tasks_list` |
| `methods::params` | All request/response param types |
| `methods::discovery` | `DefaultAgentDiscovery`, `AgentDiscovery` trait |
| `services::task_storage` | `TaskStorage` trait, `InMemoryTaskStorage` |
| `streaming` | `StreamResponse`, `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent` |

## A2A error codes

```rust
use a2a_protocol_core::a2a_error_codes;

a2a_error_codes::TASK_NOT_FOUND           // -32001
a2a_error_codes::TASK_NOT_CANCELABLE      // -32002
a2a_error_codes::PUSH_NOTIFICATION_NOT_SUPPORTED // -32003
a2a_error_codes::UNSUPPORTED_OPERATION    // -32004
a2a_error_codes::CONTENT_TYPE_NOT_SUPPORTED // -32005
a2a_error_codes::INVALID_AGENT_RESPONSE   // -32006
a2a_error_codes::EXTENDED_AGENT_CARD_NOT_CONFIGURED // -32007
// Platform extensions (-32050 range):
a2a_error_codes::AUTHENTICATION_FAILED   // -32050
a2a_error_codes::AUTHORIZATION_FAILED    // -32051
a2a_error_codes::RATE_LIMIT_EXCEEDED     // -32052
a2a_error_codes::CAPACITY_EXCEEDED       // -32053
```

## Design constraints

- **No HTTP, no sockets.** This crate compiles to pure WASM (`wasm32-unknown-unknown`) with zero platform deps.
- **Sync-only public API.** Async is handled by the server/client adapters above this layer.
- **Handler functions are not re-exported at the crate root.** Access them via `methods::messaging::handle_*` and `methods::tasks::handle_*`.
