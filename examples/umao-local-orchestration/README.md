# UMAO Local Orchestration Demo

Run a UMAO coordination graph against real A2A agents on localhost — zero cloud, zero mocks.

## Quick start

Open three terminals:

```bash
# Tab 1: Start researcher agent
cargo run -p umao-local-orchestration --bin researcher -- --port 3001

# Tab 2: Start synthesizer agent
cargo run -p umao-local-orchestration --bin synthesizer -- --port 3002

# Tab 3: Run the orchestrator with a graph
cargo run -p umao-local-orchestration -- --graph graphs/research-pipeline.json
```

## Graphs

| Graph | Description |
|-------|-------------|
| `research-pipeline.json` | Select → 2x Delegate (researcher + synthesizer) → Aggregate |
| `sequential-handoff.json` | Researcher → Synthesizer pipeline |

## Custom agents

Register additional agents with `--agent name=endpoint`:

```bash
cargo run -p umao-local-orchestration -- \
  --graph graphs/research-pipeline.json \
  --agent custom-agent=http://localhost:4000
```

---

For LLM-powered graph planning and smart agent orchestration, try [promptfleet cloud](https://promptfleet.io) (free tier).
