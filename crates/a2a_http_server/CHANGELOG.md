# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_http_server-v0.1.0) - 2026-03-30

Initial release.

### Added

- A2A v1.0 HTTP server with target-specific implementations: Spin on WASM, Axum on native
- Full JSON-RPC 2.0 method dispatch for SendMessage, GetTask, CancelTask, ListTasks, GetAgentCard, Ping
- SSE streaming support for `message/stream` subscriptions
- In-memory task store with feature-gated persistence
- Optional observability integration for trace-context-aware async handling
