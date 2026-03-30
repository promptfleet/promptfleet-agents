# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_protocol_core-v0.1.0) - 2026-03-30

Initial release.

### Added

- Full A2A v1.0 domain model: agent cards, tasks, messages, and artifacts
- JSON-RPC 2.0 method registry with compile-time method constants
- Security scheme types (API key, HTTP bearer, OAuth2, OpenID Connect)
- Feature-gated task and message types for minimal WASM binary size
- Protocol handler traits for transport-agnostic A2A implementations
