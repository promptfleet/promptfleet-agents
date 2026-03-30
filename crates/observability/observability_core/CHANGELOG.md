# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf_observability_core-v0.1.0) - 2026-03-30

Initial release.

### Added

- Core observability types: log levels, metric descriptors, and trace context
- W3C Trace Context propagation (traceparent/tracestate parsing and generation)
- Plugin trait surface for pluggable logging, metrics, and tracing backends
- Batching and flush primitives for metric and log export
- Resource attribute management with reserved-key protection
