# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_http_client-v0.1.0) - 2026-03-30

Initial release.

### Added

- A2A JSON-RPC HTTP client with unified API for WASM (Spin SDK) and native (reqwest) targets
- Activation-aware retry logic for SpinKube cold-start and scale-to-zero scenarios
- Support for all A2A v1.0 methods: SendMessage, GetTask, CancelTask, ListTasks, GetAgentCard, Ping
