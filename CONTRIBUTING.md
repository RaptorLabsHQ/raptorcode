# Contributing To RaptorCode

Date: 2026-05-15
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Contribution Standards

- Keep changes small, reviewable, and focused.
- Prefer safe Rust and explicit error handling.
- Avoid new dependencies unless they clearly reduce risk or complexity.
- Update relevant documentation when behavior changes.
- Preserve developer control over commit, merge, push, and destructive actions.

## Development Expectations

- Run formatting, linting, and tests when Rust code exists.
- Document security-sensitive assumptions.
- Do not commit generated files unless they are required for the project.
- Do not include secrets, tokens, local credentials, or machine-specific paths.

## Licensing

Contributions are accepted under `MIT OR Apache-2.0`, matching the repository license.

## Current Project State

The repository includes a Rust workspace and CLI binary named `raptor`. The v0.1 release candidate supports a local-first supervised workflow with DeepSeek chat, role worktrees, planning artifacts, implementation proposal artifacts, read-only review/diff inspection, supervised tests, explicit merge control, and a simple read-only TUI dashboard.

DeepSeek is the only implemented execution provider in v0.1. Do not add Phase 2 providers, MCP, cloud features, desktop UI, team features, plugin systems, marketplace features, policy enforcement hooks, or enterprise policy systems before BOB explicitly reopens that scope.
