# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_llm_tools-v0.1.0) - 2026-03-30

Initial release.

### Added

- `ToolRegistry` for collecting and managing LLM tool definitions
- `registry_from!` macro for compile-time tool registration from `#[llm_tool]` functions
- JSON Schema generation for tool parameters via `schemars`
- Name-based tool dispatch with optional context-aware executors
- WASM-safe implementation with no dynamic registration overhead
