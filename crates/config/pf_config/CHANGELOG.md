# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_config-v0.1.0) - 2026-03-30

Initial release.

### Added

- Layered configuration loader merging JSON files, environment variables, and optional `.env`
- Deep-merge semantics for nested configuration objects
- Environment variable coercion with configurable prefix and nesting rules
- Target-agnostic API suitable for both native and WASM environments
