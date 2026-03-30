# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_observability-v0.1.0) - 2026-03-30

### Other

- Refactor crate names for consistency and clarity
- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Stabilize observability stack and enhance API security by fixing bugs and adding tests. Key changes include resolving a string interning issue, removing circular feature flags, ensuring panic handler idempotency, and hardening the ResourceAttributeManager against reserved key overwrites. Added 47 new tests across multiple crates to improve coverage and reliability.
- Add AI changelogs for recent updates, including the introduction of `Duration::from_mins` and `from_hours` for better time handling in Rust 1.91+, and removal of redundant `Future` prelude imports in Rust 2024. Update Cargo.toml files across multiple crates to reflect the new Rust edition. Ensure all tests pass after modifications.
- Enhance observability features by introducing context-aware futures for async operations. Update A2A HTTP and WASM servers to utilize new context management, ensuring trace context is preserved across async calls. Add tests for W3C trace context handling in WASM server. Refactor observability configuration to include metrics enabling options and improve metric export functionality in OTLP collector client.
- Add duration handling for export timeout and push interval in ObservabilityConfig; introduce tests for configuration roundtrips and parallel initialization safety. Update Prometheus and OTEL plugins to use new configuration fields and improve metric handling.
- Refactor code for improved readability and consistency; apply formatting changes across multiple files, including test assertions and error handling. Enhance clarity in JSON-RPC responses and method implementations.
- Refactor project structure and dependencies; remove unused WASM example, update Cargo.toml for agent_sdk, and enhance A2A HTTP server with new JSON-RPC methods. Add tests for environment variable handling and improve deep merge functionality in config management.
- init commit 2
