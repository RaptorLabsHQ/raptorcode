# Onboarding

Date: 2026-05-18
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
- `templates/`: v0.1 role prompt templates for swarm planning.
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
- Chat command: `DEEPSEEK_API_KEY=<key> cargo run -p raptor-cli -- chat "Explain this repository"`
- Agent list command: `cargo run -p raptor-cli -- agent list`
- Agent show command: `cargo run -p raptor-cli -- agent show <id>`
- Agent create command: `cargo run -p raptor-cli -- agent create --role worker`
- Agent policy command: `cargo run -p raptor-cli -- agent policy`
- Task command: `cargo run -p raptor-cli -- task "describe the supervised task"`
- Swarm command: `cargo run -p raptor-cli -- swarm "Add GitHub OAuth to this Next.js app"`
- Swarm implementation proposal command: `cargo run -p raptor-cli -- swarm implement <task-id> frontend-agent`
- Task list command: `cargo run -p raptor-cli -- task list`
- Task show command: `cargo run -p raptor-cli -- task show <id>`
- Task abandon command: `cargo run -p raptor-cli -- task abandon <id>`
- Task cleanup command: `cargo run -p raptor-cli -- task cleanup <id>`
- Review command: `cargo run -p raptor-cli -- review <task-id>`
- Swarm role review command: `cargo run -p raptor-cli -- review <task-id> frontend-agent`
- Diff command: `cargo run -p raptor-cli -- diff <task-id>`
- Swarm role diff command: `cargo run -p raptor-cli -- diff <task-id> frontend-agent`
- Test command: `cargo run -p raptor-cli -- test <task-id>`
- Explicit test command: `cargo run -p raptor-cli -- test <task-id> -- cargo test`
- Merge dry-run command: `cargo run -p raptor-cli -- merge <task-id>`
- Merge execution command: `cargo run -p raptor-cli -- merge <task-id> --execute`
- Doctor command: `cargo run -p raptor-cli -- doctor`

## Task Workflow Notes

`raptor provider list` reads `raptor.toml` when present and lists provider metadata without network calls or model execution. `raptor chat "message"` calls DeepSeek when `DEEPSEEK_API_KEY` is set and prints the assistant response without executing it or writing files. If the key is missing, it directs the developer to `https://platform.deepseek.com/api_keys`. `raptor agent create` writes read-only local agent session metadata under `.raptor/agents/` only. `raptor agent list` and `raptor agent show <id>` inspect persisted agent session metadata. `raptor agent policy` displays the current deny-by-default permission model. Persisted agent sessions and permission policy were implemented early but are frozen until after v0.1. No autonomous coding loop, shell execution, policy enforcement hook, or automatic source-file modification exists yet.

`raptor task "description"` must be run inside a git repository. It creates an isolated git worktree under `.raptor/worktrees/` and writes metadata under `.raptor/tasks/`. It does not start autonomous agents or modify the main worktree.

`raptor swarm "description"` must be run inside a git repository. It creates one task id, four role branches, four role worktrees under `.raptor/worktrees/<task-id>/`, and swarm metadata under `.raptor/swarms/<task-id>.toml`. The roles are `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`. If `DEEPSEEK_API_KEY` is set, it calls DeepSeek once per role and writes `RAPTOR_PLAN.md` inside each assigned role worktree. If the key is missing, it keeps the worktrees and metadata, records `blocked_missing_deepseek_api_key`, and directs the developer to `https://platform.deepseek.com/api_keys`. It does not execute model output, modify application source files, run tests, merge, or push.

`raptor swarm implement <task-id> <role>` generates a supervised implementation proposal for one selected role. It validates the swarm metadata and selected role worktree, reads that role's `RAPTOR_PLAN.md`, calls DeepSeek once, writes `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside the selected role worktree, and updates swarm metadata with implementation status and proposal path. It does not execute model output, apply patches, modify application source files, run tests, merge, or push.

`raptor task list` and `raptor task show <id>` are read-only inspection commands for persisted task metadata. `raptor task abandon <id>` marks an unmerged task as abandoned without deleting metadata or the worktree. `raptor task cleanup <id>` validates the stored worktree path and deletes only the associated worktree after exact `CLEANUP <task-id>` confirmation. `raptor review <task-id>` and `raptor diff <task-id>` inspect the associated task worktree when task metadata exists; when no single-task metadata exists, they load swarm metadata and inspect all role worktrees. `raptor review <task-id> <role>` and `raptor diff <task-id> <role>` inspect one swarm role worktree. Review and diff never stage, commit, merge, delete, write metadata, or modify files. `raptor test <task-id>` runs detected tests in validated task or swarm role worktrees. Detection checks `Cargo.toml`, `package.json` with `scripts.test`, and `Makefile` with a `test` target. Developers can pass an explicit test command after `--`; only test-oriented commands are accepted, and commands are executed through structured arguments without a shell. RaptorCode does not install dependencies, call models, execute model output, update metadata, commit, merge, or push during test runs. `raptor merge <task-id>` performs merge preflight dry-run checks only. `raptor merge <task-id> --execute` requires typed confirmation, performs a supervised local merge for committed task branch changes, and records task status as `merged` or `merge_conflict`.

## Current Contributor Task

The `task` workflow creates isolated single-task worktree infrastructure and local task metadata without launching autonomous agents. The `swarm` workflow creates the v0.1 multi-agent worktree structure, metadata, planning artifacts, supervised proposal artifacts, read-only role-aware review/diff inspection, and supervised test execution. Provider execution exists only for DeepSeek chat completion through `raptor chat`, DeepSeek planning through `raptor swarm`, and DeepSeek implementation proposal generation through `raptor swarm implement`. Persisted read-only agent session metadata and a deny-by-default permission policy foundation are in place, but they remain frozen until after v0.1. Existing task metadata and task or swarm worktree changes can be inspected with `raptor task list`, `raptor task show <id>`, `raptor review <task-id> [role]`, and `raptor diff <task-id> [role]`. Tests can be run with `raptor test <task-id>`. Merge preflight, typed confirmation, supervised local merge execution, lifecycle status updates, abandonment, and safe worktree cleanup are in place for single-task metadata.

The next implementation step is `PROMPT 23 - Implement simple Ratatui dashboard`.
