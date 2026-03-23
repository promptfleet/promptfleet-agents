# 📋 AI Changelogs

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
