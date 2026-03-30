# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_llm_client-v0.1.0) - 2026-03-30

Initial release.

### Added

- Provider-agnostic `LlmClient` facade for chat completions
- OpenAI-compatible and Anthropic wire-format drivers
- SSE streaming with automatic `StreamStart` deduplication
- WASM-compatible async HTTP transport built on `protocol_transport_core`
- Configurable retry, timeout, and auth policies
