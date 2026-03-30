# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_app_ports-v0.1.0) - 2026-03-30

### Other

- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Enhance A2A Protocol documentation and testing framework. Introduce `a2a_app_ports` and `a2a_http_client` crates with comprehensive README files detailing usage, features, and integration. Implement `pf_test_harness` for local OpenAI-compatible mock testing, including new scenario integration tests for `LlmClient`. Update dependencies in `Cargo.toml` and ensure all tests pass across modules, improving overall reliability and coverage.
- Add AI changelogs for recent updates, including the introduction of `Duration::from_mins` and `from_hours` for better time handling in Rust 1.91+, and removal of redundant `Future` prelude imports in Rust 2024. Update Cargo.toml files across multiple crates to reflect the new Rust edition. Ensure all tests pass after modifications.
- init commit 2
