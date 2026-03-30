# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_rpc_macros-v0.1.0) - 2026-03-30

### Other

- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Enhance A2A Protocol documentation and testing framework. Introduce `a2a_app_ports` and `a2a_http_client` crates with comprehensive README files detailing usage, features, and integration. Implement `pf_test_harness` for local OpenAI-compatible mock testing, including new scenario integration tests for `LlmClient`. Update dependencies in `Cargo.toml` and ensure all tests pass across modules, improving overall reliability and coverage.
- Refactor observability handling in A2A HTTP server to conditionally manage async responses based on the observability feature flag. Update Cargo.toml in a2a_rpc_macros to disable doctests and enhance method argument parsing with a new helper function. Adjust task store logic to refine conditional compilation for WASM and server features.
- Add AI changelogs for recent updates, including the introduction of `Duration::from_mins` and `from_hours` for better time handling in Rust 1.91+, and removal of redundant `Future` prelude imports in Rust 2024. Update Cargo.toml files across multiple crates to reflect the new Rust edition. Ensure all tests pass after modifications.
- Refactor code for improved readability and consistency; apply formatting changes across multiple files, including test assertions and error handling. Enhance clarity in JSON-RPC responses and method implementations.
- Refactor project structure and dependencies; remove unused WASM example, update Cargo.toml for agent_sdk, and enhance A2A HTTP server with new JSON-RPC methods. Add tests for environment variable handling and improve deep merge functionality in config management.
- init commit 2
