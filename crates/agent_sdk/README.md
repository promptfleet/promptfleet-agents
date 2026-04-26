# Agent SDK

`agent_sdk` is a runtime-first Rust SDK for building PromptFleet agents and composing protocol adapters around them.

The core contract is:

- build an [`Agent`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.Agent.html) with [`AgentBuilder`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.AgentBuilder.html) (or [`Agent::new_runtime`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.Agent.html#method.new_runtime) for minimal defaults)
- register skills with [`Agent::add_skill`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.Agent.html#method.add_skill) / [`SkillEntryBuilder::handler`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.SkillEntryBuilder.html#method.handler), or the shorthand [`Agent::skill`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.Agent.html#method.skill)
- add A2A with `agent_sdk::a2a`
- add native host composition with `AgentHostBuilder`

## Architecture

- `AgentBuilder` builds the protocol-neutral runtime
- `A2aApp` wraps an `Agent` as an A2A application
- `A2aClient` talks to remote A2A agents
- `AgentHostBuilder` composes adapters around one runtime
- `agui` exposes AG-UI streaming and native app hosting

Current target support:

| Surface | Native | WASM |
| --- | --- | --- |
| `AgentBuilder` | yes | yes |
| `A2aApp` | yes | yes |
| `A2aClient` | yes | target-gated |
| `AgentHostBuilder` A2A hosting | yes | yes |
| `AgentHostBuilder` AG-UI hosting | yes | no |
| `AgUiApp` hosting | yes | no |

AG-UI hosting is native-only in the current release pass. WASM host parity is implemented for A2A only.

## Quick Start

### Runtime Builder

```rust,no_run
use agent_sdk::{AgentBuilder, SdkResult};

fn build_agent() -> SdkResult<()> {
    let agent = AgentBuilder::from_config_path("agent.json")?.build()?;
    let _ = agent;
    Ok(())
}
```

### Skills (fluent, handler optional)

```rust,no_run
use agent_sdk::{AgentBuilder, SdkResult};
use serde_json::json;

fn build_agent() -> SdkResult<()> {
    let mut agent = AgentBuilder::new("my-agent")?.build()?;
    agent
        .add_skill("echo")
        .handler(|params| async move { Ok(json!({ "echo": params })) })
        .register()?;
    Ok(())
}
```

### Structured I/O with CloudEvents

```rust,no_run
#[cfg(all(not(target_arch = "wasm32"), feature = "structured-io"))]
async fn structured_example() -> Result<(), agent_sdk::SdkError> {
    use agent_sdk::{AgentBuilder, CloudEventEnvelope, StructuredInput};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AlertSignal {
        alert_id: String,
        severity: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct AnalysisOutput {
        alert_id: String,
        disposition: String,
        confidence: f32,
    }

    let mut agent = AgentBuilder::new("analysis-agent")?.build()?;

    // Configure your real LLM runtime here.
    // agent
    //     .configure_llm_runtime(client, model, tools, system_message, None, None)?
    //     .with_structured_output::<AnalysisOutput>("analysis_output", "analysis_output")?;

    let incoming = CloudEventEnvelope::new_json(
        "com.example.alert_signal",
        "urn:promptfleet:alerts",
        AlertSignal {
            alert_id: "alert-1".to_string(),
            severity: "critical".to_string(),
        },
    );

    let result = agent
        .run_structured::<_, AnalysisOutput>(StructuredInput::from_cloudevent(incoming)?)
        .await?;

    let outgoing = result.into_cloud_event(
        "com.example.analysis_output",
        "urn:promptfleet:analysis-agent",
    );

    assert_eq!(
        outgoing.dataschema.as_deref(),
        Some("urn:promptfleet:schema:analysis_output")
    );
    let _ = outgoing;
    Ok(())
}
```

By default, structured output contracts stamp outbound CloudEvents with
`dataschema = "urn:promptfleet:schema:<schema_name>"`. Override it with
`StructuredOutputContract::with_dataschema(...)` when you need a different URI.

### Which features?

- **Runtime + tools only:** `agent-core`
- **Built-in LLM tool loop:** add `llm-engine`
- **Typed structured input/output:** add `structured-io`
- **A2A wire + server:** add `a2a-server` / `a2a-client` as needed
- **AG-UI streaming (native):** add `event-stream` and use `AgentHostBuilder::with_agui`
- **Opinionated bundle:** `pf-agent` pulls the common platform stack; trim with default features off if you need a smaller graph

### Native Host Composition

```rust,no_run
# #[cfg(all(not(target_arch = "wasm32"), feature = "dual-agent"))]
# {
use agent_sdk::{AgentBuilder, AgentHostBuilder, SdkResult};
use agent_sdk::agui::AgUiConfig;

# fn example() -> SdkResult<()> {
let agent = AgentBuilder::from_config_path("agent.json")?.build()?;
let router = AgentHostBuilder::new(agent)
    .with_a2a()
    .with_agui(AgUiConfig::default())
    .build_router()?;

let _ = router;
# Ok(())
# }
# }
```

### WASM A2A Host

```rust,no_run
# #[cfg(all(target_arch = "wasm32", feature = "a2a-server", feature = "config-loader"))]
# {
use agent_sdk::{AgentBuilder, AgentHostBuilder, SdkResult};

# async fn example(req: spin_sdk::http::Request) -> SdkResult<spin_sdk::http::Response> {
let agent = AgentBuilder::from_config_path("agent.json")?.build()?;
let host = AgentHostBuilder::new(agent).with_a2a().build()?;
host.serve_async(req).await
# }
# }
```

## Features

Core runtime:

- `agent-core`
- `llm-engine`
- `context-window`
- `interactive-tools`
- `structured-io`

A2A:

- `a2a-client`
- `a2a-server`
- `a2a-tools`
- `spec-compliant`
- `file-handling`

Hosting and streaming:

- `event-stream`
- `agui-stream`
- `agui-agent`
- `a2a-agent`
- `dual-agent`
- `pf-agent`
- `sub-agents`

Operational:

- `config-loader`
- `agent-observability`
- `auto-observability`
- `redis-storage`
- `wasm-optimized`

Maintenance-only:

- `test-support`
- `mcp-e2e`

## Examples

Minimal examples live under the crate’s [`examples/`](./examples/) directory (relative to `crates/agent_sdk`), including `structured_cloud_event.rs` for the typed structured I/O flow.
