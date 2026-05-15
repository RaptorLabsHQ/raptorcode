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
- `raptor task "description"` safe task workflow foundation.
- `raptor task list`.
- `raptor task show <id>`.
- `raptor review <task-id>`.
- `raptor diff <task-id>`.
- `raptor merge <task-id>` preflight dry run.
- Local task metadata and worktree state under `.raptor/`.

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
- No commits, merges, or pushes are performed by `raptor task`.
- `raptor review` and `raptor diff` are read-only inspection commands.
- `raptor merge` runs preflight checks and prints a dry-run summary only; no merge is executed.

Known early gap:

- If task metadata persistence fails after git worktree creation, manual cleanup may be required until an explicit cleanup workflow is implemented.

## Command Examples

Run the CLI through Cargo during development:

```bash
cargo run -p raptor-cli -- --version
cargo run -p raptor-cli -- doctor
cargo run -p raptor-cli -- init
cargo run -p raptor-cli -- task "describe the supervised task"
cargo run -p raptor-cli -- task list
cargo run -p raptor-cli -- task show <id>
cargo run -p raptor-cli -- review <task-id>
cargo run -p raptor-cli -- diff <task-id>
cargo run -p raptor-cli -- merge <task-id>
```

Expected installed binary form:

```bash
raptor --version
raptor doctor
raptor init
raptor task "describe the supervised task"
raptor task list
raptor task show <id>
raptor review <task-id>
raptor diff <task-id>
raptor merge <task-id>
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
|   |-- raptor-store/      # Local config and task metadata persistence
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

`.raptor/` is ignored by git and reserved for local runtime state.

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
