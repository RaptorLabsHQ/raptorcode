# Tech Stack

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Primary Language

Rust is the default implementation language for RaptorCode because the project needs reliable CLI behavior, safe systems programming practices, explicit errors, and maintainable local tooling.

## Planned Components

- CLI: Rust command-line interface.
- TUI: Rust terminal user interface.
- Orchestrator: Rust application layer for multi-agent workflow control.
- Git integration: local git repository and worktree operations.
- Documentation: Markdown files in the repository root and `docs/`.

## Dependency Policy

- Prefer the Rust standard library where practical.
- Add crates only when they provide clear value.
- Keep dependency count low for security and maintenance.
- Document meaningful dependency decisions.

## Current Status

No `Cargo.toml`, Rust crates, or Rust source files exist yet. This document defines intended direction, not implemented code.
