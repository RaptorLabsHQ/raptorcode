# Onboarding

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Repository Orientation

- `README.md`: short project summary.
- `AGENTS.md`: operating instructions for coding agents working in this repository.
- `LICENSE`: dual license terms, `MIT OR Apache-2.0`.
- `ROADMAP.md`: V1 milestones and non-goals.
- `SECURITY.md`: vulnerability reporting and security priorities.
- `CONTRIBUTING.md`: contribution expectations.
- `docs/`: architecture, security, tech stack, project management, and onboarding notes.

## Local Development

Rust code has not been added yet. After the Rust workspace is created, onboarding should include:

- Required Rust toolchain version.
- Build commands.
- Test commands.
- Lint and format commands.
- Example CLI and TUI usage.

## First Contributor Task

The next implementation step is to create a minimal Rust workspace for the CLI foundation without implementing agent orchestration yet.
