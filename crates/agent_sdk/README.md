# Agent SDK

`agent_sdk` is a runtime-first Rust SDK for building PromptFleet agents and composing protocol adapters around them.

The core contract is:

- build an [`Agent`](https://docs.rs/agent_sdk/latest/agent_sdk/struct.Agent.html)
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

Minimal examples live under `src/agent_sdk/examples/`.
