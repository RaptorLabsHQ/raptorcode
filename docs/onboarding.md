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
- `docs/`: architecture, security, tech stack, task workflow, project management, and onboarding notes.
- `.raptor/`: ignored local runtime state for task metadata and worktrees when task workflows are used.

## Local Development

Use the stable Rust toolchain.

- Format: `cargo fmt`
- Check: `cargo check`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets --all-features`
- Version command: `cargo run -p raptor-cli -- --version`
- Init command: `cargo run -p raptor-cli -- init`
- Task command: `cargo run -p raptor-cli -- task "describe the supervised task"`
- Task list command: `cargo run -p raptor-cli -- task list`
- Task show command: `cargo run -p raptor-cli -- task show <id>`
- Review command: `cargo run -p raptor-cli -- review <task-id>`
- Diff command: `cargo run -p raptor-cli -- diff <task-id>`
- Merge dry-run command: `cargo run -p raptor-cli -- merge <task-id>`
- Merge confirmation scaffold: `cargo run -p raptor-cli -- merge <task-id> --execute`
- Doctor command: `cargo run -p raptor-cli -- doctor`

## Task Workflow Notes

`raptor task "description"` must be run inside a git repository. It creates an isolated git worktree under `.raptor/worktrees/` and writes metadata under `.raptor/tasks/`. It does not start autonomous agents or modify the main worktree.

`raptor task list` and `raptor task show <id>` are read-only inspection commands for persisted task metadata. `raptor review <task-id>` and `raptor diff <task-id>` inspect the associated task worktree without staging, committing, merging, deleting, or modifying files. `raptor merge <task-id>` performs merge preflight dry-run checks only. `raptor merge <task-id> --execute` adds typed confirmation scaffolding and still performs no merge.

## Current Contributor Task

The `task` workflow now creates isolated worktree infrastructure and local task metadata without launching autonomous agents. Task metadata and task worktree changes can be inspected with `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, and `raptor diff <task-id>`. Merge preflight and typed confirmation scaffolding are in place. The next implementation step is supervised merge execution.
