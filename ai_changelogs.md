# 📋 AI Changelogs

## 2026-03-24 — Observability stack test stabilization and API hardening

### Changes
- **Fixed `structured_logging` string interning bug**: `intern_string()` called `.to_string()` on the symbol index (returning `"0"`) instead of resolving the actual string via `interner.resolve(sym)`. Also fixed hit/miss stats race condition with double-checked locking under write lock.
- **Fixed circular feature flags** in `structured_logging/Cargo.toml`: `fast-paths → performance-optimized → fast-paths` and `scoped-context → correlation-enhanced → scoped-context` cycles removed.
- **Fixed panic handler idempotency**: `PerformanceExtension::new()` now silently succeeds if the panic handler is already installed (OnceLock), making it test-safe across multiple instances.
- **Fixed fast-path buffer cloning**: `log_llm_request_fast` and `log_a2a_message_fast` now clone the result before returning the buffer to the pool, instead of cloning the buffer for the pool and returning the original (wasted allocation).
- **Hardened `ResourceAttributeManager`**: Custom attributes can no longer overwrite reserved `service.*` keys. Reserved keys are filtered at construction, `add_attribute()`, and `remove_attribute()`.
- **Added tests for `observability_core/src/error.rs`**: All 10 error variants, constructors, Display, Clone, Debug, From<serde_json::Error>, Result type alias.
- **Added tests for `observability_core/src/ports.rs`**: TransportPort batch default, MetricsPort batch routing, ContextPort CRUD, FormatterPort JSON output, BatchingPort lifecycle, StandardLoggingPort init/enabled.
- **Added tests for `observability/src/semconv.rs`**: Allowlist filtering (keep, drop, order preservation, empty input), allowlist content assertions (expected keys present, high-cardinality keys absent), stability tests for all span/attr/metric/value constants.
- **Added tests for `otel/src/resource_attributes.rs`**: Standard attributes always present, custom merge, reserved key protection at construction and mutation, accessor correctness, empty custom attributes.
- **Added tests for `structured_logging/src/performance.rs`**: Zero-denominator stats ratios, buffer pool exhaustion, A2A fast-path (with and without duration), string interning hit/miss stats, process_entry field interning, PerformanceManager stats aggregation and reset.

### Files modified
- `crates/observability/structured_logging/src/performance.rs` — Fixed interning bug, buffer cloning, added 7 new tests
- `crates/observability/structured_logging/Cargo.toml` — Removed circular feature cycles
- `crates/observability/structured_logging/src/extension.rs` — Made panic handler install idempotent
- `crates/observability/observability_core/src/error.rs` — Added 5 tests
- `crates/observability/observability_core/src/ports.rs` — Added 8 tests for all port traits
- `crates/observability/observability/src/semconv.rs` — Added 11 tests for allowlist and constant stability
- `crates/observability/otel/src/resource_attributes.rs` — Fixed reserved key overwrite, added 8 tests

### Tests
- `cargo test -p observability_core --all-features`: **50 passed** (was 37, +13 new)
- `cargo test -p structured_logging --all-features`: **37 passed** (was 28 pass + 2 fail, +7 new, 2 bugs fixed)
- `cargo test -p prometheus@0.1.0 --all-features`: **25 passed** (unchanged, already well-covered)
- `cargo test -p observability --no-default-features --features serde,config,logging`: **30 passed**
- `cargo test -p observability --all-features`: **32 passed** (was 21, +11 new)
- `cargo test -p otel --features otel-2025,auto-instrumentation,structured-logging,grpc-tonic`: **34 passed** (was 26, +8 new)

### Notes
- Total new tests added: **47** across 5 crates
- Total bugs fixed: **4** (string interning, circular features, panic idempotency, buffer cloning)
- Total API hardening: **1** (ResourceAttributeManager reserved key protection)
- Prometheus crate already had strong coverage (25 tests); no additional tests needed per plan guidance ("stop once high-risk public behavior is locked")

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
