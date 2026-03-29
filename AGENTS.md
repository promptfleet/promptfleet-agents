# General rule
Evaluate user intent once at chat start and show:
```
---
user intent: <intent>
---
```

If intent is planning → follow Planning rule.

## Repo Identity

This is **`promptfleet-agents`** — the **public OSS** Rust crate library. It contains the shared crate ecosystem: SDK, A2A protocol, LLM client, observability, macros, config, and test harness. All crates live under `crates/`, 24 workspace members.

This repo is a standalone library. Crates here must be self-contained and must NOT depend on any external proprietary packages.

## Planning rule
a) Planning is code-grounded deterministic work — no direct coding until user approves.
b) No auto-switch to plan mode until user allows.
c) No file writes until user asks.
d) Use Tavily/Fetch/context7 for third-party library grounding before formalizing plan.

## Workspace Architecture (24 crates, `crates/`)

| Area | Key crates | Role |
|------|-----------|------|
| **SDK / Core** | `agent_sdk`, `agent_core`, `pf_macros`, `pf-types`, `pf_config` | Agent types, SDK facade, proc macros, config |
| **A2A Protocol** | `a2a_protocol_core`, `a2a_http_client`, `a2a_http_server`, `a2a_app_ports`, `a2a_rpc_macros`, `protocol_transport_core` | A2A domain, transport, JSON-RPC |
| **LLM** | `llm_client`, `llm_context_core`, `llm_tools`, `llm_tool_macros`, `tool_web_search` | Provider-agnostic LLM, tool registry |
| **Observability** | `observability`, `observability_core`, `structured_logging`, `otel`, `prometheus` | Logging, tracing, metrics |
| **Support** | `foundation_utils`, `mcp_protocol` | Utilities, MCP protocol |
| **Test** | `pf_test_harness` | Native-only test harness for streaming, SSE, A2A routes |

## WASM Compatibility (MANDATORY)

All crates in this repo (except `pf_test_harness` and `examples/`) **must** compile to `wasm32-wasip1` (WASI Preview 1).

Quick check:
```bash
cargo check --target wasm32-wasip1 -p <crate-name>
```

Exceptions:
- `pf_test_harness` — native-only, uses tokio/axum for test infrastructure
- `examples/a2a-echo-native` — native example
- `examples/a2a-echo-wasm` — WASM example (excluded from workspace, build separately)

## Test Creation Policy

**Quality over quantity.** Every test must justify its existence.

- **Cover**: happy path, critical edge cases, error/failure paths. Skip trivial or redundant cases.
- **Unit tests**: pure logic, no I/O. Fast, isolated, deterministic.
- **Integration tests**: cross-crate or cross-boundary interactions. Real types, minimal mocking.
- **No mock workarounds**: if you can't test it without faking half the system, restructure the code.
- **Name tests descriptively**: `test_<what>_<condition>_<expected>` (e.g. `test_parse_empty_input_returns_error`).
- **One assertion focus per test** — multiple asserts OK if testing one logical outcome.

## Tests Before Docs

1. **Make changes**
2. **Run relevant tests** — `cargo test -p <crate>`
3. **Verify tests pass** — do not proceed if any test fails
4. **Update `ai_changelogs.md`** — only after tests pass
5. **Commit** (if requested)

## FMT

- never use cargo fmt without user permission
