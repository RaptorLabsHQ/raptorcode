# RaptorCode

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator.

RaptorCode V1 is not a full terminal emulator replacement. It is focused on supervised AI coding workflows, isolated task worktrees, reviewable changes, and explicit developer control.

## Status

RaptorCode is in early foundation development. v0.1 is not released yet.

The active Phase 1 product scope is:

```text
CLI + TUI + Agent Orchestrator
```

The core intended v0.1 demo command is:

```bash
raptor swarm "Add GitHub OAuth to this Next.js app"
```

No Phase 2 features should be developed before v0.1 is complete, tagged, documented, and launch-ready.

Implemented foundation:

- Rust Cargo workspace.
- `raptor` CLI binary from `raptor-cli`.
- CLI command router for the planned V1 command surface.
- `raptor --version`.
- `raptor init`.
- `raptor doctor`.
- `raptor provider list`.
- `raptor chat "message"` DeepSeek completion smoke test.
- `raptor agent list`.
- `raptor agent show <id>`.
- `raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]`.
- `raptor agent policy`.
- `raptor task "description"` safe task workflow foundation.
- `raptor task list`.
- `raptor task show <id>`.
- `raptor task abandon <id>`.
- `raptor task cleanup <id>`.
- `raptor review <task-id> [role]`.
- `raptor diff <task-id> [role]`.
- `raptor merge <task-id>` preflight dry run.
- `raptor merge <task-id> --execute` supervised local merge execution.
- `raptor swarm "task description"` multi-agent worktree MVP.
- `raptor swarm "task description"` DeepSeek planning that writes `RAPTOR_PLAN.md` per role when `DEEPSEEK_API_KEY` is set.
- `raptor swarm implement <task-id> <role>` supervised implementation proposal generation.
- DeepSeek provider execution through `DEEPSEEK_API_KEY`.
- Provider metadata catalog for DeepSeek first, plus future OpenAI/Codex, Claude, Gemini, and local model boundaries.
- Read-only persisted local agent session model for future multi-agent orchestration.
- Deny-by-default permission policy foundation for future agent tools.
- Local task, agent, and worktree state under `.raptor/`.

Stubbed commands:

- `raptor test`

## v0.1 Scope Lock

The controlling source of truth is `COMPLETE_PROJECT_TECHNICAL_BLUEPRINT_SPECIFICATIONS_GUIDELINES_ROADMAP.md`.

Until v0.1 ships, implementation must directly support:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

Do not add advanced persisted agent sessions, advanced permission policy systems, policy enforcement hooks, cloud features, plugin systems, MCP, desktop terminal features, marketplace features, team features, or additional model providers before v0.1.

The persisted agent sessions and permission policy added in commit `483135d7e8e8d25de726874eaddfaa7338f77fe0` are implemented early but frozen until after v0.1. They should not be expanded on the v0.1 path.

## Safety Model

RaptorCode assumes AI coding agents can make incorrect or unsafe changes. The system is designed to keep developers in control before work affects the main worktree or remote repository.

Current controls:

- Task descriptions must be non-empty.
- Task ids are deterministic and path-safe.
- Git worktrees are isolated under `.raptor/worktrees/`.
- Task metadata is stored under `.raptor/tasks/`.
- Swarm metadata is stored under `.raptor/swarms/`.
- Existing worktree paths and task metadata are not overwritten.
- Branch and worktree names are validated before git operations.
- Path traversal is rejected.
- Git commands are executed through structured process arguments.
- No autonomous agents are launched by the current task workflow.
- Provider inspection performs no external requests and no model calls.
- `raptor chat` sends only the fixed system prompt and user-provided chat message to DeepSeek.
- `raptor chat` does not send repository contents, `.env` files, secrets, task metadata, or worktree files.
- `raptor chat` does not execute model output, run shell commands, or write source files.
- `raptor swarm` sends only the task description and role template to DeepSeek when `DEEPSEEK_API_KEY` is set.
- `raptor swarm` writes only `RAPTOR_PLAN.md` planning artifacts inside assigned role worktrees.
- `raptor swarm` blocks planning without failing worktree creation when `DEEPSEEK_API_KEY` is missing.
- Swarm plans are AI-generated and must be reviewed before implementation.
- Swarm planning does not execute model output, apply patches, edit application source files, run tests, merge, commit, or push.
- `raptor swarm implement <task-id> <role>` sends only the task description, selected role, and that role's `RAPTOR_PLAN.md` to DeepSeek.
- `raptor swarm implement <task-id> <role>` writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside the selected role worktree.
- Implementation proposals are AI-generated and must be reviewed before source changes are applied manually.
- Implementation proposal generation does not execute model output, apply patches, edit application source files, run tests, merge, commit, or push.
- Agent session creation writes metadata only under `.raptor/agents/`.
- Agent inspection is read-only and loads only validated `.raptor/agents/<agent-id>.toml` metadata.
- Agent sessions do not modify source files, execute shell commands, make network requests, or call AI models in this foundation.
- The default permission policy denies file writes, shell execution, network access, model execution, and dangerous commands.
- No commits, merges, or pushes are performed by `raptor task`.
- `raptor review` and `raptor diff` are read-only inspection commands for single-task worktrees and swarm role worktrees.
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
cargo run -p raptor-cli -- chat "Explain the next implementation step"
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
cargo run -p raptor-cli -- review <task-id> frontend-agent
cargo run -p raptor-cli -- diff <task-id>
cargo run -p raptor-cli -- diff <task-id> frontend-agent
cargo run -p raptor-cli -- merge <task-id>
cargo run -p raptor-cli -- merge <task-id> --execute
cargo run -p raptor-cli -- swarm "Add GitHub OAuth to this Next.js app"
cargo run -p raptor-cli -- swarm implement <task-id> frontend-agent
```

Expected installed binary form:

```bash
raptor --version
raptor doctor
raptor init
raptor provider list
raptor chat "Explain the next implementation step"
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
raptor review <task-id> frontend-agent
raptor diff <task-id>
raptor diff <task-id> frontend-agent
raptor merge <task-id>
raptor merge <task-id> --execute
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor swarm implement <task-id> frontend-agent
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

## Swarm Workflow

`raptor swarm "task description"` is the v0.1 multi-agent worktree MVP. It must be run inside a git repository.

It currently:

- Creates one deterministic task id from the description.
- Creates four role branches:
  - `raptor/<task-id>/frontend-agent`
  - `raptor/<task-id>/backend-agent`
  - `raptor/<task-id>/test-agent`
  - `raptor/<task-id>/security-agent`
- Creates four isolated role worktrees:
  - `.raptor/worktrees/<task-id>/frontend-agent`
  - `.raptor/worktrees/<task-id>/backend-agent`
  - `.raptor/worktrees/<task-id>/test-agent`
  - `.raptor/worktrees/<task-id>/security-agent`
- Writes swarm metadata under `.raptor/swarms/<task-id>.toml`.
- Records provider `deepseek`, model `deepseek-chat`, planning status, per-agent planning status, and plan file paths when generated.
- With `DEEPSEEK_API_KEY` set, calls DeepSeek once per role and writes:
  - `.raptor/worktrees/<task-id>/frontend-agent/RAPTOR_PLAN.md`
  - `.raptor/worktrees/<task-id>/backend-agent/RAPTOR_PLAN.md`
  - `.raptor/worktrees/<task-id>/test-agent/RAPTOR_PLAN.md`
  - `.raptor/worktrees/<task-id>/security-agent/RAPTOR_PLAN.md`
- Without `DEEPSEEK_API_KEY`, preserves worktrees and metadata, marks planning as `blocked_missing_deepseek_api_key`, and prints `https://platform.deepseek.com/api_keys`.

Swarm planning is planning-only. It does not modify application source files inside the role worktrees, execute model output, run tests, merge, commit, or push.

`raptor swarm implement <task-id> <role>` is the minimal supervised worktree file modification path for v0.1. It:

- Accepts only `frontend-agent`, `backend-agent`, `test-agent`, or `security-agent`.
- Loads `.raptor/swarms/<task-id>.toml`.
- Validates the selected role worktree under `.raptor/worktrees/<task-id>/<role>/`.
- Requires `DEEPSEEK_API_KEY`.
- Reads that role's `RAPTOR_PLAN.md`.
- Calls DeepSeek once for the selected role.
- Writes `.raptor/worktrees/<task-id>/<role>/RAPTOR_IMPLEMENTATION_PROPOSAL.md`.
- Records per-role implementation status and proposal file path in swarm metadata.

The proposal file preserves any patch-like model output as text only. RaptorCode does not apply patches, modify application source code, execute model output, run tests, merge, commit, or push in this command.

Role-aware review and diff are read-only:

- `raptor review <task-id>` inspects a single-task worktree when task metadata exists, otherwise it loads swarm metadata and fans out across all role worktrees.
- `raptor review <task-id> <role>` inspects one swarm role worktree.
- `raptor diff <task-id>` prints single-task diffs when task metadata exists, otherwise it loads swarm metadata and prints diffs for all role worktrees.
- `raptor diff <task-id> <role>` prints diffs for one swarm role worktree.
- Valid swarm roles are `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- Review and diff never stage, commit, merge, delete, write metadata, modify files, execute model output, run tests, or push.

Task inspection commands are read-only:

- `raptor task list` prints persisted task metadata from `.raptor/tasks/`.
- `raptor task show <id>` validates the task id and prints details for one task.
- `raptor review <task-id> [role]` prints task or swarm role metadata, git status, and changed files from validated worktrees.
- `raptor diff <task-id> [role]` prints changed files plus unstaged and staged git diffs from validated worktrees.
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

DeepSeek is the only provider with execution before v0.1. `raptor chat "message"` reads the API key from `DEEPSEEK_API_KEY`, sends a non-streaming chat completion request to DeepSeek, and prints the assistant response.

If `DEEPSEEK_API_KEY` is missing, the command reports that the key is required and directs the developer to:

```text
https://platform.deepseek.com/api_keys
```

API keys must not be stored in `raptor.toml`, committed to the repository, printed, or logged. Other provider integrations remain Phase 2 and must not be implemented before v0.1.

`raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]` creates local session metadata only under `.raptor/agents/`. Roles currently include `architect`, `reviewer`, and `worker`. Task, provider, model, and agent ids are validated before persistence, and duplicate deterministic session ids are refused.

`raptor agent list` and `raptor agent show <id>` inspect persisted read-only agent session state. The session model supports future task binding, provider/model metadata, lifecycle state, messages, events, and audit entries, but there is no autonomous coding loop yet.

`raptor agent policy` prints the current default permission policy. This exists early but is frozen until after v0.1 and must not grow into a policy enforcement system before launch.

## v0.1 Launch UX Requirement

Running plain `raptor` must eventually show a premium terminal launch screen before the developer prompt becomes ready.

The launch screen must include Raptor Labs, `https://raptorlabs.dev`, RaptorCode `v0.1`, a premium robust CLI logo treatment, and a short greeting from the Raptor Labs team. Brand colors are white, near-white light gray, and vivid high-saturation intense red aligned with RaptorLabs.dev. Exact hex values are blocked until BOB adds the brand reference image to the repository or provides exact hex values.

The launch flow must also onboard the developer to DeepSeek. If no DeepSeek API key is available, the CLI must direct the developer to:

```text
https://platform.deepseek.com/api_keys
```

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
|   |-- raptor-store/      # Local config, task metadata, swarm metadata, and agent metadata persistence
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

`.raptor/` is ignored by git and reserved for local runtime state, including task metadata, swarm metadata, agent metadata, and worktrees.

## Development

Use the stable Rust toolchain.

```bash
cargo fmt
cargo check
cargo clippy --all-targets --all-features
cargo test
```

The workspace uses internal crates plus `reqwest`, `serde`, and `serde_json` for the DeepSeek HTTPS chat completion path and its no-live-network tests.

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
