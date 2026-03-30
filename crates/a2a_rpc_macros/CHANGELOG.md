# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_rpc_macros-v0.1.0) - 2026-03-30

Initial release.

### Added

- `#[a2a_rpc]` proc-macro for declaring A2A JSON-RPC handlers with compile-time registration
- Extension method support for custom agent-specific RPC commands
- Optimized for small WASM binaries — no dynamic dispatch tables
