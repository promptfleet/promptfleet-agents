# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/a2a_app_ports-v0.1.0) - 2026-03-30

Initial release.

### Added

- `A2AAppPort` trait boundary for clean-architecture separation between HTTP server and application logic
- Async variant for non-blocking agent card and message handling
- Minimal dependency surface to keep server crates decoupled from SDK internals
