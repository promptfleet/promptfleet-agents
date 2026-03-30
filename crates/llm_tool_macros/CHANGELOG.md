# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_llm_tool_macros-v0.1.0) - 2026-03-30

Initial release.

### Added

- `#[llm_tool]` attribute macro to register Rust functions as LLM-callable tools
- Automatic JSON Schema generation for tool parameters via `schemars`
- Integration with `pf_llm_tools::ToolRegistry` for compile-time tool collection
