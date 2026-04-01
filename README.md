# PromptFleet Agents

Rust crates for building AI agents, LLM apps, and agentic workflows — WASM and native.

---

## What is PromptFleet Agents?


A collection of **independent, composable Rust crates** for AI agent development. Most library crates work standalone or compose with others, and target both **WASM** (Fermyon Spin / SpinKube) and **native** (Axum / Tokio) runtimes. 


Exceptions: `pf_test_harness` is native-only, and examples are intentionally target-specific.

The ecosystem covers LLM integration, agent-to-agent communication (A2A), user-facing streaming (AG-UI), Model Context Protocol (MCP), observability, and tool calling. Use one crate for a single concern, or bring in `agent_sdk` to compose the full stack.

**[Docs, guides, and API reference →](https://promptfleet.github.io/promptfleet-agents/)**

---

## Pick What You Need

| I want to... | Crate | Cargo.toml |
|---|---|---|
| Call LLMs from WASM or native | `pf_llm_client` | `llm_client = { package = "pf_llm_client", version = "0.1" }` |
| Manage token budgets and history | `pf_llm_context_core` | `llm_context_core = { package = "pf_llm_context_core", version = "0.1" }` |
| Register Rust functions as LLM tools | `pf_llm_tools` + `pf_llm_tool_macros` | `llm_tools = { package = "pf_llm_tools", version = "0.1" }` |
| Connect to MCP servers | `pf_mcp_protocol` | `mcp_protocol = { package = "pf_mcp_protocol", version = "0.1", features = ["client"] }` |
| Add structured logging / OTEL / Prometheus | `pf_observability` | `observability = { package = "pf_observability", version = "0.1", features = ["otel"] }` |
| Build an A2A-compliant agent | `pf_agent_sdk` | `agent_sdk = { package = "pf_agent_sdk", version = "0.1", features = ["a2a-agent"] }` |
| Stream agent output to a UI (AG-UI) | `pf_agent_sdk` | `agent_sdk = { package = "pf_agent_sdk", version = "0.1", features = ["agui-agent"] }` |
| Serve A2A + AG-UI on one host | `pf_agent_sdk` | `agent_sdk = { package = "pf_agent_sdk", version = "0.1", features = ["dual-agent"] }` |
| Build a full agentic workflow | `pf_agent_sdk` | `agent_sdk = { package = "pf_agent_sdk", version = "0.1", features = ["pf-agent"] }` |

Published packages use `pf_` prefixes for crates.io uniqueness. In code you use the short dependency key (e.g. `llm_client`, `agent_sdk`).

---

## Ecosystem

### LLM

Build LLM-powered apps without the SDK. These crates work standalone in any Rust project.

| Crate | What it does | WASM |
|---|---|---|
| `pf_llm_client` | Provider-agnostic LLM client — OpenAI, Anthropic, Azure. Streaming SSE on native, buffered on WASM. Wire-format abstraction, model profiles. | Yes |
| `pf_llm_context_core` | Token estimation, budget tracking, history strategies, long-term memory traits. | Yes |
| `pf_llm_tools` | Lightweight tool registry and name-indexed executor for function-calling workflows. | Yes |
| `pf_llm_tool_macros` | `#[llm_tool]` proc-macro — turns plain Rust functions into LLM tools with auto-generated JSON Schema. | Yes |
| `pf_tool_web_search` | Ready-made LLM tool: web search via Tavily API. Dual-target HTTP. | Yes |

**Composition:** `llm_client` + `llm_tools` + `llm_context_core` = full LLM pipeline with tool calling and token management.

<details>
<summary>LLM client in 5 lines</summary>

```rust
use llm_client::{LlmClient, WireFormat, auth::ApiKeyAuth, ChatMessage, LlmRequest};

let client = LlmClient::builder(WireFormat::OpenAiCompat)
    .base_url("https://api.openai.com/v1")
    .auth(ApiKeyAuth::new(std::env::var("OPENAI_API_KEY")?))
    .build()?;

let resp = client.chat(LlmRequest {
    model: "gpt-4o-mini".into(),
    messages: vec![ChatMessage { role: "user".into(), content: Some("Hello".into()), ..Default::default() }],
    ..Default::default()
}).await?;
```

Same code works on WASM and native. Switch to Anthropic with `WireFormat::AnthropicMessages`, or use `LlmClient::azure_openai_builder(...)` for Azure.
</details>

### A2A Protocol (Google Agent-to-Agent v1.0)

Full implementation of the [Google A2A Protocol](https://google.github.io/A2A/). Use these crates to build agents that discover and delegate tasks to each other.

| Crate | What it does | WASM |
|---|---|---|
| `a2a_protocol_core` | Pure A2A domain layer — AgentCard, Task, Message, Artifact, method registry, in-memory task storage. No transport dependency. | Yes |
| `a2a_http_server` | A2A HTTP server — Spin SDK on WASM, Axum on native. JSON-RPC dispatch, SSE streaming, agent discovery. | Yes |
| `a2a_http_client` | A2A HTTP client — Spin SDK on WASM, Reqwest on native. Activation-aware retries for KEDA cold starts. | Yes |
| `a2a_app_ports` | Clean-architecture port traits (`A2AAppPort`) decoupling HTTP server from application logic. | Yes |
| `protocol_transport_core` | JSON-RPC 2.0 wire types, streaming primitives, header forwarding. Shared by A2A and MCP. | Yes |

**Composition:** `a2a_protocol_core` + `a2a_http_server` = standalone A2A-compliant server without the SDK.

### AG-UI (Agent-to-UI Streaming)

Stream agent execution to user interfaces via SSE. Built into `agent_sdk` (native only).

| Feature flag | What it enables |
|---|---|
| `agui-stream` | SSE streaming surface — `AgUiStream`, `AgUiStreamDriver`, IO events, run status, enrichers. |
| `agui-agent` | Full AG-UI agent runtime — LLM orchestration + AG-UI streaming. |
| `dual-agent` | A2A + AG-UI on a single Axum router via `AgentHostBuilder`. |

**Composition:** `agent_sdk[dual-agent]` = one binary serving both machine-to-machine (A2A) and user-facing (AG-UI) interfaces.

<details>
<summary>Dual-protocol host in 4 lines</summary>

```rust
use agent_sdk::{AgentBuilder, AgentHostBuilder, agui::AgUiConfig};

let agent = AgentBuilder::from_config_path("agent.json")?.build()?;
let router = AgentHostBuilder::new(agent)
    .with_a2a()                         // A2A JSON-RPC + agent card
    .with_agui(AgUiConfig::default())   // AG-UI SSE streaming
    .build_router()?;                   // single Axum router
```

One builder, one binary — A2A for agent-to-agent communication, AG-UI for user-facing streaming, both on the same port.
</details>

### MCP (Model Context Protocol)

| Crate | What it does | WASM |
|---|---|---|
| `pf_mcp_protocol` | Full MCP implementation — JSON-RPC 2.0, tool discovery, tool execution, memory operations, OAuth 2.1 Bearer auth. FastMCP compatible. HTTP+SSE and Streamable HTTP transports. | Yes |

**Standalone:** use as an MCP client to connect your agent to any MCP server, or run your own MCP server.

### Observability

Production-grade observability that works in WASM. Use these crates in any Rust project — no agent SDK required.

| Crate | What it does | WASM |
|---|---|---|
| `pf_observability_core` | Core traits and types for structured logging, metrics, spans, trace context. | Yes |
| `pf_observability` | Facade that routes to backends (OTEL, Prometheus). One import, batteries included. | Yes |
| `pf_structured_logging` | Enhanced structured logging — string interning, buffer pools, fast paths, correlation context. | Yes |
| `pf_otel` | OpenTelemetry extension optimized for SpinKube WASM. Auto-instrumentation, gRPC export. | Yes |
| `pf_prometheus` | Prometheus metrics — federation, pushgateway, cardinality reduction, recording rules. | Yes |

**Composition:** `observability[otel]` + `structured_logging` = production logging with distributed traces.

<details>
<summary>Observability in one call</summary>

```rust
use observability::Obs;

// One-liner init from environment (OTEL endpoint, log level, etc.)
let _obs = Obs::init_from_env()?;

// Now standard log macros are structured and enriched automatically
log::info!("agent started");

// Optional: add domain context for auto-enriched telemetry
structured_logging::llm_context!("gpt-4o");
log::info!("LLM request sent"); // automatically tagged with model, component, operation
structured_logging::clear_context!();
```

Three tiers: (1) just `Obs::init()` + standard `log` macros, (2) add context macros for auto-enrichment, (3) use `emit_*` functions for custom metrics.
</details>

### Agent SDK

The composition layer. Brings everything above together via feature flags — you only pay for what you enable.

| Crate | What it does | WASM |
|---|---|---|
| `pf_agent_sdk` | `AgentBuilder`, skills, tools, handlers, config loading. Optional A2A server/client, AG-UI streaming, LLM orchestration, MCP client, observability — all behind feature flags. | Yes |
| `pf_agent_core` | Protocol-agnostic domain types — messages, tasks, artifacts, roles, content parts. Lightweight (`serde` only). | Yes |

<details>
<summary>Agent from config file</summary>

```rust
use agent_sdk::{AgentBuilder, Agent};

let agent: Agent = AgentBuilder::from_config_path("agent.json")?
    .build()?;
```

Or configure programmatically with `AgentConfig::new(...)`, add skills via `agent.add_skill("name").description("...").register()?`, and set handlers. The builder validates everything at build time.
</details>

### Foundation

| Crate | What it does | WASM |
|---|---|---|
| `pf_config` | Layered config loader — JSON, env vars, dotenv, Cargo.toml metadata. | Yes |
| `pf_foundation_utils` | RAII patterns, scoped operations, resource management. Zero dependencies. | Yes |
| `pf-types` | Lightweight primitives — `AgentSlug`, name normalization, validation. | Yes |

---

## Quick Start (native A2A agent)

This uses the full SDK. For standalone crate usage, see the per-crate docs.

```toml
[dependencies]
agent_sdk = { package = "pf_agent_sdk", version = "0.1", features = ["a2a-agent"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

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

```sh
cargo run --example a2a-echo-native
```

The server exposes:
- `POST /jsonrpc` — A2A JSON-RPC 2.0 endpoint
- `GET /.well-known/agent-card.json` — agent discovery card

---

## WASM + Native

All library crates except `pf_test_harness` compile to both `wasm32-wasip1` (Fermyon Spin / SpinKube on Kubernetes) and native (Axum / Tokio). The same code, the same APIs — the transport layer adapts automatically:

| | WASM | Native |
|---|---|---|
| HTTP server | Spin SDK handler | Axum router |
| HTTP client | Spin outbound HTTP | Reqwest |
| Streaming | Buffered | Incremental SSE |
| AG-UI | — | Axum + SSE |

Target selection is compile-time via `cfg(target_arch = "wasm32")` — no runtime overhead.

Native-only / target-specific exceptions:
- `pf_test_harness` is native-only and exists for Tokio/Axum-based seam and integration testing.
- `examples/a2a-echo-native` is a native-only example and is included in the workspace.
- `examples/a2a-echo-wasm` is the WASM example and is built separately from the workspace.
- `examples/umao-local-orchestration` is a native-only example excluded from the workspace because it depends on sibling-path crates.

If you are validating WASM compatibility with `cargo check --target wasm32-wasip1`, exclude `pf_test_harness` and the native-only examples from blanket workspace checks.

---

## Protocol Compliance

| Protocol | Version | Status |
|---|---|---|
| [Google A2A](https://google.github.io/A2A/) | v1.0 | Full — Ping, GetAgentCard, SendMessage, SendStreamingMessage, GetTask, CancelTask, ListTasks |
| [MCP](https://modelcontextprotocol.io/) | 2025-06-18 | Tool discovery, tool execution, memory ops, OAuth 2.1 |
| JSON-RPC | 2.0 | Foundation for both A2A and MCP |
| OpenTelemetry | OTLP | Traces, metrics, auto-instrumentation |

---

## License

Apache-2.0
