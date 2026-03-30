# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_test_harness-v0.1.0) - 2026-03-30

Initial release.

### Added

- `LlmScenario` for deterministic LLM turn sequences (text, tool calls, errors, reasoning)
- `TestPipeline` for end-to-end streaming tests: scenario → engine → mapper → SSE capture
- `SseCollector` / `SseCapture` for asserting SSE frame sequences, event names, and JSON paths
- A2A HTTP helpers: JSON-RPC body builders and `collect_send_subscribe_sse` for axum router testing
- Native-only crate (tokio/axum) — not compiled for WASM targets
