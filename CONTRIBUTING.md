# Contributing to promptfleet-agents

Thanks for your interest in improving the PromptFleet agent framework.

## Before You Open a Pull Request

- Open an issue first for significant API, protocol, or crate-structure changes.
- Keep pull requests focused and easy to review.
- All library crates (except `pf_test_harness`) must compile to `wasm32-wasip1`. Run `cargo check --target wasm32-wasip1 -p <crate>` before submitting.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for PR titles (`feat:`, `fix:`, `docs:`, `chore:`, etc.).

## Pull Request Expectations

- Explain what changed and why.
- Reference the relevant crate, module, or protocol section when possible.
- Keep public APIs, docs, and tests in sync.
- Be responsive to review feedback.

## Review

All changes require maintainer review before merge.

Changes affecting public APIs, cross-crate behavior, or A2A protocol compliance may require deeper review than internal refactors or editorial fixes.

## Community Standards

Please follow the [Code of Conduct](CODE_OF_CONDUCT.md) in all project spaces.

For general repository questions, contact `oss-support@promptfleet.ai`.
