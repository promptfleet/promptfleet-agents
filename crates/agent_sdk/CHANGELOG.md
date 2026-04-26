# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/promptfleet/promptfleet-agents/releases/tag/v0.1.1) - 2026-04-26

### Added

- Protocol-free structured input/output helpers under the `structured-io` feature, including typed `StructuredInput`, `StructuredOutputContract`, and `StructuredRunResult`
- CloudEvents helpers for typed payload envelopes, schema derivation, and conventional `dataschema` URIs
- Fluent runtime configuration via `configure_llm_runtime(...)? .with_structured_output::<T>(...)`

### Changed

- `StructuredRunResult::into_cloud_event(...)` now stamps `dataschema` automatically from the configured structured output contract
- CloudEvents helper APIs are now owned by `agent_sdk`, so the SDK can be released without a separate `pf-events` crates.io dependency

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/v0.1.0) - 2026-03-30

Initial release.

### Added

- `AgentBuilder` for zero-boilerplate agent construction from JSON config
- `SkillEntryBuilder` for fluent skill registration with optional LLM tool handlers
- Feature-gated A2A and AG-UI protocol adapters
- Built-in LLM engine with streaming, tool-call loops, and context management
- Dual-target runtime: Spin component on WASM, Axum server on native
- Protocol-neutral message/task lifecycle with automatic state transitions
