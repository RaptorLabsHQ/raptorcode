# Onboarding

Date: 2026-05-17
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
- `.raptor/`: ignored local runtime state for task metadata, agent metadata, and worktrees when workflows are used.

## Local Development

Use the stable Rust toolchain.

- Format: `cargo fmt`
- Check: `cargo check`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets --all-features`
- Version command: `cargo run -p raptor-cli -- --version`
- Init command: `cargo run -p raptor-cli -- init`
- Provider list command: `cargo run -p raptor-cli -- provider list`
- Agent list command: `cargo run -p raptor-cli -- agent list`
- Agent show command: `cargo run -p raptor-cli -- agent show <id>`
- Agent create command: `cargo run -p raptor-cli -- agent create --role worker`
- Agent policy command: `cargo run -p raptor-cli -- agent policy`
- Task command: `cargo run -p raptor-cli -- task "describe the supervised task"`
- Task list command: `cargo run -p raptor-cli -- task list`
- Task show command: `cargo run -p raptor-cli -- task show <id>`
- Task abandon command: `cargo run -p raptor-cli -- task abandon <id>`
- Task cleanup command: `cargo run -p raptor-cli -- task cleanup <id>`
- Review command: `cargo run -p raptor-cli -- review <task-id>`
- Diff command: `cargo run -p raptor-cli -- diff <task-id>`
- Merge dry-run command: `cargo run -p raptor-cli -- merge <task-id>`
- Merge execution command: `cargo run -p raptor-cli -- merge <task-id> --execute`
- Doctor command: `cargo run -p raptor-cli -- doctor`

## Task Workflow Notes

`raptor provider list` reads `raptor.toml` when present and lists provider metadata without network calls or model execution. `raptor agent create` writes read-only local agent session metadata under `.raptor/agents/` only. `raptor agent list` and `raptor agent show <id>` inspect persisted agent session metadata. `raptor agent policy` displays the current deny-by-default permission model. No autonomous coding loop, shell execution, network request, model call, or agent source-file modification exists yet.

`raptor task "description"` must be run inside a git repository. It creates an isolated git worktree under `.raptor/worktrees/` and writes metadata under `.raptor/tasks/`. It does not start autonomous agents or modify the main worktree.

`raptor task list` and `raptor task show <id>` are read-only inspection commands for persisted task metadata. `raptor task abandon <id>` marks an unmerged task as abandoned without deleting metadata or the worktree. `raptor task cleanup <id>` validates the stored worktree path and deletes only the associated worktree after exact `CLEANUP <task-id>` confirmation. `raptor review <task-id>` and `raptor diff <task-id>` inspect the associated task worktree without staging, committing, merging, deleting, or modifying files. `raptor merge <task-id>` performs merge preflight dry-run checks only. `raptor merge <task-id> --execute` requires typed confirmation, performs a supervised local merge for committed task branch changes, and records task status as `merged` or `merge_conflict`.

## Current Contributor Task

The `task` workflow now creates isolated worktree infrastructure and local task metadata without launching autonomous agents. Provider metadata, persisted read-only agent session metadata, and a deny-by-default permission policy foundation are in place for future orchestration. Task metadata and task worktree changes can be inspected with `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, and `raptor diff <task-id>`. Merge preflight, typed confirmation, supervised local merge execution, lifecycle status updates, abandonment, and safe worktree cleanup are in place. The next implementation step is policy enforcement around future model invocation scaffolding.
