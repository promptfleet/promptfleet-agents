# a2a_app_ports

Application-layer port traits for A2A HTTP servers (clean architecture boundary).

The server infrastructure (`a2a_http_server`) delegates **`SendMessage`** through `handle_send_message` / `handle_send_message_async`, and **`GetAgentCard`** through **`build_agent_card`** on the same traits. Other standard methods (`GetTask`, `CancelTask`, `ListTasks`, `GetExtendedAgentCard`, etc.) are handled by the shared protocol layer (`A2AProtocol`, plus optional task storage) and do not get per-method hooks on these ports.

## Traits

### `A2AAppPort` (synchronous)

```rust
use a2a_app_ports::A2AAppPort;
use a2a_protocol_core::{AgentCard, methods::params::{SendMessageRequest, SendMessageResponse}, A2AResult};

struct MyAgent;

impl A2AAppPort for MyAgent {
    fn build_agent_card(&self) -> AgentCard {
        AgentCard::new("my-agent")
            .with_capability("SendMessage", "Process user messages")
    }

    fn handle_send_message(&self, params: SendMessageRequest) -> A2AResult<SendMessageResponse> {
        let text = params.message.get_text_content();
        // ... application logic ...
        Ok(SendMessageResponse::Message(/* ... */))
    }
}
```

### `A2AAppPortAsync` (async, WASM-compatible)

```rust
use a2a_app_ports::{A2AAppPortAsync, AppFuture};

impl A2AAppPortAsync for MyAgent {
    fn build_agent_card(&self) -> AgentCard { /* ... */ }

    fn handle_send_message_async<'a>(&'a self, params: SendMessageRequest) -> AppFuture<'a> {
        Box::pin(async move {
            // async application logic
            Ok(SendMessageResponse::Message(/* ... */))
        })
    }
}
```

## Wiring into the server

```rust
use a2a_http_server::A2AHttpServer;

let server = A2AHttpServer::with_app_port(Arc::new(MyAgent));
// or for async:
let server = A2AHttpServer::with_app_port_async(Arc::new(MyAgent));
```

## Design notes

- `A2AAppPort` and `A2AAppPortAsync` cover **message handling** and **agent card** materialization for this server. Task and extended-card RPCs stay in the protocol stack; calling arbitrary methods on *other* agents is a separate concern (`a2a_http_client`).
- `build_agent_card` is called on every `GetAgentCard` request — keep it cheap (no I/O).
- The `AppFuture` type alias is `Send`-bounded on native and `!Send` on WASM to match each runtime's requirements.
