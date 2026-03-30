# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/promptfleet/promptfleet-agents/releases/tag/pf-types-v0.1.0) - 2026-03-30

### Other

- Enhance default implementations and refactor code for clarity
- Refactor imports and improve code organization across multiple crates
- Refactor project structure and enhance documentation
- Add AI changelogs for recent updates, including the introduction of `Duration::from_mins` and `from_hours` for better time handling in Rust 1.91+, and removal of redundant `Future` prelude imports in Rust 2024. Update Cargo.toml files across multiple crates to reflect the new Rust edition. Ensure all tests pass after modifications.
- Update pf-types crate to version 0.2.0, removing the ulid dependency and refactoring the library to focus on lightweight naming and validation primitives. Enhance documentation and simplify the code structure for better usability in the PromptFleet agent ecosystem.
- init commit 2
