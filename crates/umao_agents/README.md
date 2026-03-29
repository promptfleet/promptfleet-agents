# umao_agents

UMAO coordination for promptfleet A2A agents — `AsyncNodeExecutor` backed by `agent_sdk`.

## Specification Lineage

| Layer | Project | License |
|-------|---------|---------|
| Specification | [UMAO spec](https://github.com/umao-spec/umao-spec) | CSL 1.0 |
| Reference impl | [umao-rs](https://github.com/umao-spec/umao-rs) | Apache 2.0 |
| This crate | `umao_agents` | Apache 2.0 |

This crate bridges the vendor-neutral UMAO orchestration engine (`umao_core` + `umao_executor` from umao-rs) with the promptfleet agent SDK, dispatching UMAO graph Delegate nodes to real A2A agents.

`promptfleet-agents` is a registered [reference implementation](https://github.com/umao-spec/umao-spec#reference-implementations) of the UMAO specification.

## Usage

```rust
use umao_agents::PromptFleetExecutor;

let mut executor = PromptFleetExecutor::new();
executor.register("researcher", "http://localhost:3001");
executor.register("synthesizer", "http://localhost:3002");
```

## Documentation

- [Crate docs](https://promptfleet.github.io/promptfleet-agents/support/umao-agents/) — API reference and dispatch table
- [UMAO concepts](https://promptfleet.github.io/promptfleet-agents/concepts/umao/) — conceptual overview
