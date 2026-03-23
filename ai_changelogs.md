# 📋 AI Changelogs

## 2026-03-23 — `llm_client` pre-OSS hardening (all workstreams A-E)

### Summary
Complete implementation of the `llm_client` pre-OSS hardening plan: provider-neutral message model, Anthropic provider with full parity, comprehensive test coverage, WASM SSE support, and agent_sdk seam cleanup.

**Workstream B — Provider-neutral message model:**
- `ChatMessage.content` changed from `String` to `Option<String>`
- Added `ToolCallRequest { id, name, arguments: Value }` type
- Added `tool_calls`, `tool_call_id`, `name` fields to `ChatMessage`
- All downstream code updated for the enriched type

**Workstream E — Anthropic provider (full parity):**
- New `AnthropicClient` with Messages API support (`/v1/messages`)
- `to_messages_payload`: system extraction, `input_schema`, tool result batching, max_tokens default
- `normalize_messages_json`: text/tool_use extraction, stop_reason mapping, usage mapping
- `parse_anthropic_chunk`: typed SSE event parser for Anthropic's event format
- Native SSE streaming via `sse_event_stream_anthropic`
- `SseParser` extended with `next_typed_event()` for `event:` field tracking

**Workstream D — WASM buffer-then-parse SSE:**
- `HttpModelClient::post_sse_buffered` for dual-target HTTP
- `sse_event_stream_from_buffer` for OpenAI-style SSE bodies
- WASM `llm_stream`/`llm_stream_raw` on both `OpenAIClient` and `AnthropicClient`
- `ClientCapabilities::streaming` set to `true` on all targets

**Workstream C — agent_sdk seam cleanup:**
- `IntoLlmInvoker`/`IntoLlmStreamInvoker` for `AnthropicClient`
- Core tool loop uses typed `ChatMessage` + `ToolCallRequest` instead of raw `json!`

**Workstreams A1-A3 — Test coverage:**
- `prepare.rs`: 0% → 96% (13 tests)
- `openai.rs`: 22% → 90% (22 tests total, 20 new)
- `model_client.rs`: 56% → 68% (7 tests)
- `anthropic.rs`: new file → 84% (20 tests)
- `stream.rs`: 88% → 89% (4 new tests)
- **Overall llm_client: 53% → 87.3%**

### Files modified
- `crates/llm/llm_client/src/types.rs` — ChatMessage enrichment, ToolCallRequest
- `crates/llm/llm_client/src/providers/openai.rs` — enriched type support, 20 new tests
- `crates/llm/llm_client/src/providers/anthropic.rs` — **new file**, full Anthropic provider
- `crates/llm/llm_client/src/providers/mod.rs` — export AnthropicClient
- `crates/llm/llm_client/src/stream.rs` — typed events, buffer-parse, Anthropic chunk parser
- `crates/llm/llm_client/src/model_client.rs` — post_sse_buffered, streaming=true, tests
- `crates/llm/llm_client/src/prepare.rs` — 13 unit tests
- `crates/llm/llm_client/src/lib.rs` — export parse_anthropic_chunk
- `crates/agent_sdk/src/agent/llm_invoker.rs` — Anthropic invoker impls
- `crates/agent_sdk/src/agent/engine/core_loop.rs` — typed message construction

### Tests
- `cargo test -p llm_client`: **95 unit + 3 integration + 1 doctest = 99 total, all pass**
- `cargo test -p agent_sdk --features llm-engine`: **190 unit + 6 integration + 13 doctests = 209 total, all pass**
- `cargo check --target wasm32-wasip1 -p llm_client`: **pass**
- `cargo llvm-cov -p llm_client`: **87.3% line coverage** (target 80%+)

## 2026-03-23 — `Duration::from_mins` / `from_hours` for minute/hour literals (Rust 1.91+)

### Summary
Replaced selected `Duration::from_secs(...)` with `from_mins` / `from_hours` for exact 60s, 600s, and 3600s literals.

### Files modified
- `crates/foundation_utils/src/resource.rs` — `600` → `from_mins(10)`, `60` → `from_mins(1)`
- `crates/a2a_http_client/src/activation.rs` — `60` → `from_mins(1)` (default + test assert)
- `crates/observability/observability_core/src/batching.rs` — `3600` → `from_hours(1)`

### Tests
- `cargo test -p a2a_http_client -p foundation_utils -p observability_core`: **passed** (incl. foundation_utils doctests)

### Notes
- Companion edits in `promptfleet-agents-cloud` are recorded in that repo’s `ai_changelogs.md`.

## 2026-03-23 — Remove redundant `Future` prelude imports (Rust 2024)

### Summary
Dropped standalone `use std::future::Future` / `use core::future::Future`; Rust 2024 prelude already includes `Future`.

### Files modified
- `crates/agent_sdk/src/agent/skill.rs`
- `crates/agent_sdk/src/callable.rs`
- `crates/agent_sdk/src/sub_agent/adapter.rs`
- `crates/a2a_app_ports/src/lib.rs` (`core::future::Future`)
- `crates/llm_context_core/src/history.rs`
- `crates/llm_context_core/src/memory.rs`
- `crates/observability/observability_core/src/context.rs`

### Tests
- `CARGO_TARGET_DIR=/tmp/pf-future-import-check cargo check -p agent_sdk -p observability_core -p llm_context_core -p a2a_app_ports`: **passed**

### Notes
- Companion edits in `promptfleet-agents-cloud` are recorded in that repo’s `ai_changelogs.md`.
