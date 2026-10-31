# Tech Stack

Date: 2026-05-18
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Primary Language

Rust is the default implementation language for RaptorCode because the project needs reliable CLI behavior, safe systems programming practices, explicit errors, and maintainable local tooling.

## Planned Components

- CLI: Rust command-line interface built by `raptor-cli` as the `raptor` binary.
- Core domain: task ids, task status, task metadata, and worktree information in `raptor-core`.
- TUI: Rust terminal user interface boundary in `raptor-tui`, using Ratatui and Crossterm for the v0.1 read-only dashboard.
- Orchestrator: Rust application layer boundary for read-only agent session state, permission policy modeling, v0.1 role templates, and future multi-agent workflow control in `raptor-agent`.
- Providers: Rust provider abstraction boundary for DeepSeek execution first. Other provider names are future metadata boundaries only and do not execute in v0.1.
- Git integration: local git repository inspection, worktree operations, read-only single-task and swarm role worktree status/diff inspection, supervised merge execution, and validated worktree cleanup in `raptor-git`.
- Store: local configuration, provider enablement parsing, task metadata persistence, swarm metadata persistence and planning status updates, agent metadata persistence, task metadata inspection, and lifecycle status updates in `raptor-store`.
- Tool diagnostics: local tooling checks plus safe test command detection and execution in `raptor-tools`.
- Templates: Markdown role templates in repository root `templates/`.
- Documentation: Markdown files in the repository root and `docs/`.

## Dependency Policy

- Prefer the Rust standard library where practical.
- Add crates only when they provide clear value.
- Keep dependency count low for security and maintenance.
- Document meaningful dependency decisions.

## Current Status

The repository contains a Cargo workspace with internal crate dependencies plus the minimal HTTP and serialization crates needed for DeepSeek execution. The implemented CLI surface includes base `raptor`, `raptor tui`, `raptor --version`, `raptor init`, `raptor doctor`, `raptor provider list`, `raptor chat "message"`, `raptor agent list`, `raptor agent show <id>`, `raptor agent create`, `raptor agent policy`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor task abandon <id>`, `raptor task cleanup <id>`, `raptor swarm "description"`, `raptor swarm implement <task-id> <role>`, `raptor review <task-id> [role]`, `raptor diff <task-id> [role]`, `raptor test <task-id>`, `raptor merge <task-id>`, and `raptor merge <task-id> --execute`.

Provider execution is synchronous and uses `reqwest` for HTTPS, `serde` for explicit request/response structs, and `serde_json` for response parsing tests and error handling. DeepSeek is the only implemented provider before v0.1. Base `raptor` renders the startup screen and checks only whether `DEEPSEEK_API_KEY` exists. `raptor tui` uses Ratatui and Crossterm for a simple read-only dashboard with no network calls. `raptor swarm "description"` uses DeepSeek for planning only and writes `RAPTOR_PLAN.md` artifacts inside role worktrees. `raptor test <task-id>` uses standard library process execution with structured arguments after validating task or swarm role worktree paths and command shape. Persisted agent sessions use the existing local store boundary and a minimal text format under `.raptor/agents/`. The permission policy foundation uses Rust enums and explicit allow/deny helpers. Persisted agent sessions and permission policy were implemented early and are frozen until after v0.1.

Before v0.1, the tech stack may grow only to support the launch path:

- DeepSeek execution.
- Minimal role-based swarm orchestration.
- Minimal worktree-scoped file modification.
- Final verification.
- Release notes final review.
- v0.1 tag after explicit approval.

Do not add provider SDKs, async runtimes, secret-management dependencies, or additional provider implementations before v0.1. `reqwest`, `serde`, and `serde_json` are present only for the DeepSeek chat completion path, test command detection, and tests.
