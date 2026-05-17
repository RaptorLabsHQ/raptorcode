# RaptorCode

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator.

RaptorCode V1 is not a full terminal emulator replacement. It is focused on supervised AI coding workflows, isolated task worktrees, reviewable changes, and explicit developer control.

## Status

RaptorCode is in early foundation development.

Implemented today:

- Rust Cargo workspace.
- `raptor` CLI binary from `raptor-cli`.
- CLI command router for the planned V1 command surface.
- `raptor --version`.
- `raptor init`.
- `raptor doctor`.
- `raptor provider list`.
- `raptor agent list`.
- `raptor agent show <id>`.
- `raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]`.
- `raptor agent policy`.
- `raptor task "description"` safe task workflow foundation.
- `raptor task list`.
- `raptor task show <id>`.
- `raptor task abandon <id>`.
- `raptor task cleanup <id>`.
- `raptor review <task-id>`.
- `raptor diff <task-id>`.
- `raptor merge <task-id>` preflight dry run.
- `raptor merge <task-id> --execute` supervised local merge execution.
- Provider metadata catalog for DeepSeek first, plus future OpenAI/Codex, Claude, Gemini, and local model boundaries.
- Read-only persisted local agent session model for future multi-agent orchestration.
- Deny-by-default permission policy foundation for future agent tools.
- Local task, agent, and worktree state under `.raptor/`.

Stubbed commands:

- `raptor chat`
- `raptor swarm`
- `raptor test`

## Safety Model

RaptorCode assumes AI coding agents can make incorrect or unsafe changes. The system is designed to keep developers in control before work affects the main worktree or remote repository.

Current controls:

- Task descriptions must be non-empty.
- Task ids are deterministic and path-safe.
- Git worktrees are isolated under `.raptor/worktrees/`.
- Task metadata is stored under `.raptor/tasks/`.
- Existing worktree paths and task metadata are not overwritten.
- Branch and worktree names are validated before git operations.
- Path traversal is rejected.
- Git commands are executed through structured process arguments.
- No autonomous agents are launched by the current task workflow.
- Provider inspection performs no external requests and no model calls.
- Agent session creation writes metadata only under `.raptor/agents/`.
- Agent inspection is read-only and loads only validated `.raptor/agents/<agent-id>.toml` metadata.
- Agent sessions do not modify source files, execute shell commands, make network requests, or call AI models in this foundation.
- The default permission policy denies file writes, shell execution, network access, model execution, and dangerous commands.
- No commits, merges, or pushes are performed by `raptor task`.
- `raptor review` and `raptor diff` are read-only inspection commands.
- `raptor merge` runs preflight checks and prints a dry-run summary only; no merge is executed.
- `raptor merge <task-id> --execute` runs the same preflight checks, requires exact typed confirmation `MERGE <task-id>`, performs a local git merge from the task branch, and records task status as `merged` or `merge_conflict`.
- `raptor task cleanup <id>` deletes only the validated task worktree after exact typed confirmation `CLEANUP <task-id>` and only for `merged`, `completed`, or `abandoned` tasks.
- Cleanup preserves task metadata, branches, and remotes.

Known early gap:

- If task metadata persistence fails before a task record is available, manual cleanup may still be required because cleanup is task-metadata driven.

## Command Examples

Run the CLI through Cargo during development:

```bash
cargo run -p raptor-cli -- --version
cargo run -p raptor-cli -- doctor
cargo run -p raptor-cli -- init
cargo run -p raptor-cli -- provider list
cargo run -p raptor-cli -- agent list
cargo run -p raptor-cli -- agent show <id>
cargo run -p raptor-cli -- agent create --role worker
cargo run -p raptor-cli -- agent policy
cargo run -p raptor-cli -- task "describe the supervised task"
cargo run -p raptor-cli -- task list
cargo run -p raptor-cli -- task show <id>
cargo run -p raptor-cli -- task abandon <id>
cargo run -p raptor-cli -- task cleanup <id>
cargo run -p raptor-cli -- review <task-id>
cargo run -p raptor-cli -- diff <task-id>
cargo run -p raptor-cli -- merge <task-id>
cargo run -p raptor-cli -- merge <task-id> --execute
```

Expected installed binary form:

```bash
raptor --version
raptor doctor
raptor init
raptor provider list
raptor agent list
raptor agent show <id>
raptor agent create --role worker
raptor agent policy
raptor task "describe the supervised task"
raptor task list
raptor task show <id>
raptor task abandon <id>
raptor task cleanup <id>
raptor review <task-id>
raptor diff <task-id>
raptor merge <task-id>
raptor merge <task-id> --execute
```

## Task Workflow

`raptor task "description"` must be run inside a git repository.

It currently:

- Detects the repository root.
- Creates a deterministic task id.
- Creates a branch named `raptor-task-<task-id>`.
- Creates an isolated worktree under `.raptor/worktrees/<task-id>`.
- Writes task metadata under `.raptor/tasks/<task-id>.toml`.
- Reports task id, status, branch, worktree path, and metadata path.

It does not launch coding agents yet.

Task inspection commands are read-only:

- `raptor task list` prints persisted task metadata from `.raptor/tasks/`.
- `raptor task show <id>` validates the task id and prints details for one task.
- `raptor review <task-id>` prints task metadata, git status, and changed files from the task worktree.
- `raptor diff <task-id>` prints unstaged and staged git diffs from the task worktree.
- `raptor merge <task-id>` validates merge preconditions and prints a dry-run summary without merging.
- `raptor merge <task-id> --execute` validates preconditions, requires `MERGE <task-id>`, and merges committed task branch changes into the current target branch.
- Successful merge execution records task status as `merged`; merge conflicts record `merge_conflict`; failed preflight does not mark a task merged.
- `raptor task abandon <id>` marks an unmerged task as `abandoned` without deleting metadata or the worktree.
- `raptor task cleanup <id>` validates the stored worktree path, prints a cleanup plan, requires `CLEANUP <task-id>`, and deletes only the associated worktree directory.

## Provider And Agent Foundation

`raptor provider list` reads `raptor.toml` when present and reports registered provider metadata. New configs created by `raptor init` include:

```toml
[providers.deepseek]
enabled = true
```

Provider support is metadata-only in this iteration. No API keys, secrets, network calls, or model execution are implemented.

`raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]` creates local session metadata only under `.raptor/agents/`. Roles currently include `architect`, `reviewer`, and `worker`. Task, provider, model, and agent ids are validated before persistence, and duplicate deterministic session ids are refused.

`raptor agent list` and `raptor agent show <id>` inspect persisted read-only agent session state. The session model supports future task binding, provider/model metadata, lifecycle state, messages, events, and audit entries, but there is no autonomous coding loop yet.

`raptor agent policy` prints the current default permission policy. V1 policy is manual approval with repository-root-only file-read modeling and denied file write, shell execution, network, model execution, and dangerous command permissions.

## Repository Layout

```text
.
|-- crates/
|   |-- raptor-agent/      # Agent lifecycle boundary
|   |-- raptor-cli/        # CLI router and raptor binary
|   |-- raptor-core/       # Core domain models
|   |-- raptor-git/        # Git repository and worktree operations
|   |-- raptor-providers/  # Future provider integration boundary
|   |-- raptor-sandbox/    # Future execution isolation boundary
|   |-- raptor-store/      # Local config, task metadata, and agent metadata persistence
|   |-- raptor-tools/      # Local tooling diagnostics
|   `-- raptor-tui/        # Future terminal UI boundary
|-- docs/
|   |-- architecture.md
|   |-- onboarding.md
|   |-- project-management.md
|   |-- security.md
|   |-- task-workflow.md
|   `-- tech-stack.md
|-- AGENTS.md
|-- CONTRIBUTING.md
|-- LICENSE
|-- ROADMAP.md
|-- SECURITY.md
`-- Cargo.toml
```

`.raptor/` is ignored by git and reserved for local runtime state, including task metadata, agent metadata, and task worktrees.

## Development

Use the stable Rust toolchain.

```bash
cargo fmt
cargo check
cargo clippy --all-targets --all-features
cargo test
```

The workspace currently uses only internal crate dependencies.

## Documentation

- [Architecture](docs/architecture.md)
- [Security Design](docs/security.md)
- [Task Workflow](docs/task-workflow.md)
- [Tech Stack](docs/tech-stack.md)
- [Onboarding](docs/onboarding.md)
- [Project Management](docs/project-management.md)
- [Roadmap](ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)

## Licensing

RaptorCode is licensed under `MIT OR Apache-2.0`.
