# Tech Stack

Date: 2026-05-17
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
- Orchestrator: Rust application layer boundary for read-only agent session state, permission policy modeling, and future multi-agent workflow control in `raptor-agent`.
- Providers: Rust provider abstraction boundary for DeepSeek first, future OpenAI/Codex, future Claude, future Gemini, and future local models in `raptor-providers`.
- Git integration: local git repository inspection, worktree operations, read-only worktree status/diff inspection, supervised merge execution, and validated worktree cleanup in `raptor-git`.
- Store: local configuration, provider enablement parsing, task metadata persistence, agent metadata persistence, task metadata inspection, and lifecycle status updates in `raptor-store`.
- Tool diagnostics: local tooling checks in `raptor-tools`.
- Documentation: Markdown files in the repository root and `docs/`.

## Dependency Policy

- Prefer the Rust standard library where practical.
- Add crates only when they provide clear value.
- Keep dependency count low for security and maintenance.
- Document meaningful dependency decisions.

## Current Status

The repository contains a Cargo workspace with internal crate dependencies only. The implemented CLI surface includes `raptor --version`, `raptor init`, `raptor doctor`, `raptor provider list`, `raptor agent list`, `raptor agent show <id>`, `raptor agent create`, `raptor agent policy`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor task abandon <id>`, `raptor task cleanup <id>`, `raptor review <task-id>`, `raptor diff <task-id>`, `raptor merge <task-id>`, and `raptor merge <task-id> --execute`.

Provider and agent foundations are synchronous, metadata-only, and dependency-free. Persisted agent sessions use the existing local store boundary and a minimal text format under `.raptor/agents/`. The permission policy foundation uses Rust enums and explicit allow/deny helpers. These components were implemented early and are frozen until after v0.1.

Before v0.1, the tech stack may grow only to support the launch path:

- DeepSeek execution.
- Minimal role-based swarm orchestration.
- Minimal worktree-scoped file modification.
- Test execution inside worktrees.
- Simple Ratatui dashboard.
- Premium terminal launch screen and DeepSeek API key onboarding.

Do not add HTTP clients, async runtimes, provider SDKs, serialization crates, or secret-management dependencies unless they are directly required for DeepSeek execution or the v0.1 demo path. Do not implement additional providers before v0.1.

The `chat`, `swarm`, and `test` commands are routed but intentionally stubbed until their supervised workflows are implemented.
