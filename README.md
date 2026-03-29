# promptfleet-agents

Rust workspace for building [A2A Protocol v1.0](https://google.github.io/A2A/) agents targeting both **Fermyon Spin (WASM)** and **native Axum/Tokio** runtimes.

---

## Crate Map

| Crate | Purpose | Primary target |
|---|---|---|
| `agent_sdk` | High-level SDK: skills, tools, A2A + AG-UI adapters | WASM + native |
| `a2a_http_server` | A2A JSON-RPC HTTP server (Spin WASM / Axum native) | WASM + native |
| `a2a_http_client` | A2A JSON-RPC HTTP client (Spin HTTP / Reqwest native) | WASM + native |
| `a2a_protocol_core` | Pure domain layer: protocol, registry, task storage, data types | WASM + native |
| `a2a_app_ports` | Port traits connecting HTTP server to application layer | WASM + native |
| `protocol_transport_core` | JSON-RPC 2.0 wire types shared across crates | WASM + native |
| `agent_core` | Low-level agent runtime primitives | WASM + native |
| `observability` | Metrics, spans, trace context propagation | WASM + native |
| `llm` | LLM client abstraction (OpenAI-compatible) | native |
| `mcp_protocol` | MCP (Model Context Protocol) types and client | native |

### Documentation site (Starlight)

The docs website lives in **`docs/`**. Run Yarn **from `docs/`** (the repo root has no `package.json`; Yarn at the root is usually **v1** and may litter the tree with a stray `yarn.lock` / `node_modules`).

```bash
cd docs
corepack enable
yarn install
yarn dev
```

If Yarn prints **Clipanion** errors mentioning **`While running --non-interactive`**, see **`docs/src/content/docs/getting-started.mdx`** (Troubleshooting section).

---

## Architecture

```
                          +---------------------------+
                          |        agent_sdk           |
                          |  Agent · A2aApp · AgUI     |
                          +----------+--------+--------+
                                     |        |
                +--------------------+        +-------------------+
                |                                                  |
   +------------+-------------+                  +----------------+----+
   |    a2a_http_server        |                  |  a2a_http_client    |
   |  WASM: Spin handler       |                  |  WASM: Spin HTTP    |
   |  Native: Axum router      |                  |  Native: Reqwest    |
   +------------+--------------+                  +----------------+----+
                |                                                  |
   +------------+--------------------------------------------------+----+
   |                        a2a_protocol_core                           |
   |   A2AProtocol · A2AMethodRegistry · TaskStorage · Data types       |
   +-----------------------------------+--------------------------------+
                                       |
                       +---------------+---------------+
                       |   protocol_transport_core     |
                       |   JsonRpcRequest/Response     |
                       +-------------------------------+
```

The server delegates `SendMessage` to the application via `a2a_app_ports::A2AAppPort`. All other methods (`GetTask`, `CancelTask`, `ListTasks`) are handled entirely by the protocol layer using `InMemoryTaskStorage` (or a custom `TaskStorage` implementation).

---

## Quick Start (native)

Add dependencies to your `Cargo.toml`:

```toml
[dependencies]
agent_sdk = { path = "crates/agent_sdk", features = ["a2a-agent"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

Implement an echo agent (`examples/a2a-echo-native/src/main.rs`):

```rust
use agent_sdk::{
    a2a::A2aApp,
    agent::{AgentConfig, MessageContext, Response, TaskContext},
    Agent,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = AgentConfig::new("echo-agent", "Echoes back user messages")
        .with_base_url("http://127.0.0.1:3000");

    let mut agent = Agent::new_with_config(config)?;
    agent
        .add_skill("echo")
        .description("Echo user messages")
        .register()?;
    agent.set_message_handler(echo_handler);

    let app = A2aApp::from_agent(agent)?;
    println!("Listening on http://127.0.0.1:3000");
    app.serve("127.0.0.1:3000").await
}

async fn echo_handler(
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
) -> agent_sdk::SdkResult<agent_sdk::agent::RuntimeResponse> {
    let input = msg_ctx.text_content.as_deref().unwrap_or("(empty)");
    Response::message_text(
        format!("Echo: {input}"),
        None,
        None,
        task_ctx.and_then(|t| t.context_id),
    )
}
```

Run it:

```sh
cargo run --example a2a-echo-native
```

The server exposes:
- `POST /jsonrpc` — A2A JSON-RPC 2.0 endpoint
- `GET /.well-known/agent-card.json` — agent discovery card

---

## Feature Flags

### `a2a_protocol_core`

| Feature | Effect |
|---|---|
| `protocol-core` | Enables data types (`Task`, `Message`, `Artifact`), task storage, and messaging methods. Required for most usage. |
| `event-stream` | Enables `SendStreamingMessage` method and `StreamResponse` SSE types. |
| `file-handling` | Treat URL/raw message parts as task-creating rather than utility responses. |
| `time-stamps` | Populate timestamp fields using `chrono::Utc::now()`. Off by default to keep WASM builds lean. |
| `all-features` | Enables all of the above. |

### `a2a_http_server`

| Feature | Effect |
|---|---|
| `event-stream` | Enables the SSE streaming endpoint and `A2AStreamingAppPort` trait. |

### `agent_sdk`

| Feature | Effect |
|---|---|
| `a2a-agent` | A2A-capable agent runtime (`A2aApp`, `A2aServer`). |
| `agui-agent` | AG-UI agent runtime. |
| `dual-agent` | Both A2A and AG-UI. |
| `a2a-server` | Lower-level A2A server access within the SDK. |
| `observability` | Trace context propagation and metrics in the A2A client. |

---

## Protocol Compatibility

Implements **Google A2A Protocol v1.0**. Supported methods:

| Method | Description |
|---|---|
| `Ping` | Health check — returns `{"pong": true}` |
| `GetAgentCard` | Returns the agent's `AgentCard` metadata |
| `GetExtendedAgentCard` | Authenticated extended card (optional auth gate) |
| `SendMessage` | Create or continue a task; short utility messages return inline |
| `SendStreamingMessage` | Start a task and subscribe to SSE status updates (`event-stream` feature) |
| `GetTask` | Retrieve task state, history, and artifacts |
| `CancelTask` | Cancel a non-terminal task |
| `ListTasks` | List tasks with optional context/status filtering and pagination |

All methods use JSON-RPC 2.0 over HTTP POST to `/jsonrpc`. The `a2a-version: 1.0` request header is expected on all calls.

---

## Running Tests

```sh
# Unit + integration tests (native)
cargo test -p a2a_protocol_core -p a2a_http_server -p a2a_http_client

# All feature combinations
cargo test -p a2a_protocol_core --features all-features

# Coverage report (requires cargo-llvm-cov)
cargo llvm-cov --package a2a_protocol_core --features all-features --summary-only
```
