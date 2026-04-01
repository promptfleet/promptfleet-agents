# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/umao_agents-v0.1.0) - 2026-04-01

### Other

- update Cargo.toml and dependencies for UMAO integration
- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Add UMAO agents crate and enhance documentation
- Enhance `umao_agents` with improved input aggregation and mock agent server functionality. Update `Cargo.toml` to include additional Tokio features. Refactor `researcher` and `synthesizer` agents to extract and summarize topics from input messages, improving response clarity. Update orchestration example to utilize the new event sink structure for better event handling.
- Implement UMAO agents for local orchestration, introducing the `umao_agents` crate. This includes a `PromptFleetExecutor` for managing A2A agent interactions, along with two agents: `researcher` and `synthesizer`, each running a minimal A2A HTTP server. Add example orchestration graphs and a demo application to showcase agent coordination. Update `Cargo.toml` and `Cargo.lock` to include new dependencies and configurations for the agents and orchestration example.
