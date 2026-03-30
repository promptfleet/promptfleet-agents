# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/v0.1.0) - 2026-03-30

### Other

- Refactor crate names for consistency and clarity
- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Enhance `agent_sdk` with comprehensive documentation updates, including detailed usage for `AgentHostBuilder`, `A2aApp`, and `AgUiConfig`. Introduce new tests for A2A health checks and agent card responses, ensuring robust functionality. Update `README.md` for clearer examples and feature descriptions. Add `tower` as a dev-dependency for improved testing capabilities. All tests pass, confirming stability and reliability.
- Enhance `agent_sdk` documentation and examples with zero warnings in rustdoc. Update `README.md` to include detailed usage of `AgentBuilder` and `SkillEntryBuilder`. Improve module documentation across various files, ensuring clarity in the API. All tests pass, confirming stability and functionality.
- Enhance `agent_sdk` with unified skill registration and improved documentation. Introduce `SkillEntryBuilder` for fluent skill registration, allowing optional handlers. Update `AgentBuilder` with new constructors and migration guidance. Refactor LLM configuration methods for clarity and consistency. Improve README and changelogs with detailed usage examples and feature descriptions. Ensure all tests pass, including new tests for skill registration and LLM policies.
- Implement deduplication of `StreamStart` events in OpenAI SSE drivers within `llm_client`. Introduce `dedupe_stream_starts` to ensure only one `StreamEvent::StreamStart` per HTTP response, addressing issues with repeated `delta.role` in certain streams. Update related functions and add new tests for deduplication behavior. Modify README to reflect this guarantee. Update dependencies in `Cargo.lock` for improved functionality.
- Update README with comprehensive documentation for the A2A Protocol v1.0 agents, including crate map, architecture overview, quick start guide, feature flags, and protocol compatibility details. Refactor A2A HTTP client and server methods to standardize the use of "Ping" instead of "pf.agent.ping". Enhance tests across various modules to improve coverage and reliability, including new tests for messaging and task handling. Remove deprecated error codes and streamline transport builder functionality.
- Add Anthropic provider support to LLM client, including a new `AnthropicClient` implementation. Enhance `ChatMessage` and `ToolCallRequest` structures for better tool call handling. Update streaming and request handling in `HttpModelClient` for compatibility with WASM. Introduce comprehensive test coverage for new features and ensure all existing tests pass.
- Refactor observability handling in A2A HTTP server to conditionally manage async responses based on the observability feature flag. Update Cargo.toml in a2a_rpc_macros to disable doctests and enhance method argument parsing with a new helper function. Adjust task store logic to refine conditional compilation for WASM and server features.
- Add AI changelogs for recent updates, including the introduction of `Duration::from_mins` and `from_hours` for better time handling in Rust 1.91+, and removal of redundant `Future` prelude imports in Rust 2024. Update Cargo.toml files across multiple crates to reflect the new Rust edition. Ensure all tests pass after modifications.
- Refactor code for improved readability and consistency; apply formatting changes across multiple files, including test assertions and error handling. Enhance clarity in JSON-RPC responses and method implementations.
- Refactor project structure and dependencies; remove unused WASM example, update Cargo.toml for agent_sdk, and enhance A2A HTTP server with new JSON-RPC methods. Add tests for environment variable handling and improve deep merge functionality in config management.
- init commit 2
