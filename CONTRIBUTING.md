# Contributing To RaptorCode

Date: 2026-05-15
Author: Tim
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

The foundation documentation exists before Rust implementation. The Rust workspace, CLI, TUI, and orchestrator crates are planned but not implemented in this documentation-only increment.
