# Onboarding

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Repository Orientation

- `README.md`: short project summary.
- `AGENTS.md`: operating instructions for coding agents working in this repository.
- `Cargo.toml`: Rust workspace manifest.
- `LICENSE`: dual license terms, `MIT OR Apache-2.0`.
- `ROADMAP.md`: V1 milestones and non-goals.
- `SECURITY.md`: vulnerability reporting and security priorities.
- `CONTRIBUTING.md`: contribution expectations.
- `crates/`: Rust crates for the CLI, core metadata, git integration, tools, and planned V1 boundaries.
- `docs/`: architecture, security, tech stack, project management, and onboarding notes.

## Local Development

Use the stable Rust toolchain.

- Format: `cargo fmt`
- Check: `cargo check`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets --all-features`
- Version command: `cargo run -p raptor-cli -- --version`
- Init command: `cargo run -p raptor-cli -- init`
- Task command: `cargo run -p raptor-cli -- task "describe the supervised task"`
- Doctor command: `cargo run -p raptor-cli -- doctor`

## Current Contributor Task

The `task` workflow now creates isolated worktree infrastructure and local task metadata without launching autonomous agents. The next implementation step is to add task listing and review inspection so developers can supervise task outputs before merge.
