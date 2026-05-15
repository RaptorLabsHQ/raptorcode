# Tech Stack

Date: 2026-05-15
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
- Git integration: local git repository inspection, worktree operations, and read-only worktree status/diff inspection in `raptor-git`.
- Store: local configuration, task metadata persistence, and task metadata inspection in `raptor-store`.
- Tool diagnostics: local tooling checks in `raptor-tools`.
- Documentation: Markdown files in the repository root and `docs/`.

## Dependency Policy

- Prefer the Rust standard library where practical.
- Add crates only when they provide clear value.
- Keep dependency count low for security and maintenance.
- Document meaningful dependency decisions.

## Current Status

The repository contains a Cargo workspace with internal crate dependencies only. The implemented CLI surface includes `raptor --version`, `raptor init`, `raptor doctor`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, and `raptor diff <task-id>`.

The `chat`, `swarm`, `test`, and `merge` commands are routed but intentionally stubbed until their supervised workflows are implemented.
