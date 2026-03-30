# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_llm_context_core-v0.1.0) - 2026-03-30

Initial release.

### Added

- Context window builder with token-budget tracking
- Configurable history strategies (sliding window, summarization ports)
- System prompt management with fixed and dynamic segments
- Long-term memory trait ports for retrieval-augmented generation
- WASM-compatible token estimation without native tokenizer dependencies
