# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/v0.1.0) - 2026-03-30

Initial release.

### Added

- `AgentBuilder` for zero-boilerplate agent construction from JSON config
- `SkillEntryBuilder` for fluent skill registration with optional LLM tool handlers
- Feature-gated A2A and AG-UI protocol adapters
- Built-in LLM engine with streaming, tool-call loops, and context management
- Dual-target runtime: Spin component on WASM, Axum server on native
- Protocol-neutral message/task lifecycle with automatic state transitions
