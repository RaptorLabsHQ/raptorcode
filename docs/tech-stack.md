# Tech Stack

Date: 2026-05-16
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Primary Language

Rust is the default implementation language for RaptorCode because the project needs reliable CLI behavior, safe systems programming practices, explicit errors, and maintainable local tooling.

## Planned Components

- CLI: Rust command-line interface built by `raptor-cli` as the `raptor` binary.
- Core domain: task ids, task status, task metadata, and worktree information in `raptor-core`.
- TUI: Rust terminal user interface boundary in `raptor-tui`.
- Orchestrator: Rust application layer boundary for multi-agent workflow control in `raptor-agent`.
- Git integration: local git repository inspection, worktree operations, read-only worktree status/diff inspection, supervised merge execution, and validated worktree cleanup in `raptor-git`.
- Store: local configuration, task metadata persistence, task metadata inspection, and lifecycle status updates in `raptor-store`.
- Tool diagnostics: local tooling checks in `raptor-tools`.
- Documentation: Markdown files in the repository root and `docs/`.

## Dependency Policy

- Prefer the Rust standard library where practical.
- Add crates only when they provide clear value.
- Keep dependency count low for security and maintenance.
- Document meaningful dependency decisions.

## Current Status

The repository contains a Cargo workspace with internal crate dependencies only. The implemented CLI surface includes `raptor --version`, `raptor init`, `raptor doctor`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor task abandon <id>`, `raptor task cleanup <id>`, `raptor review <task-id>`, `raptor diff <task-id>`, `raptor merge <task-id>`, and `raptor merge <task-id> --execute`.

The `chat`, `swarm`, and `test` commands are routed but intentionally stubbed until their supervised workflows are implemented.
