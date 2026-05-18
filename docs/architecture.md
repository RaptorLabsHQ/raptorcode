# Architecture

Date: 2026-05-18
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Architecture Goals

- Keep local developer supervision central to all agent workflows.
- Isolate agent work by default using git worktrees.
- Make repository state, agent state, and pending actions visible.
- Require explicit approval for commits, merges, pushes, and destructive filesystem operations.
- Prefer minimal Rust modules with clear boundaries.

## Planned Layers

- CLI: command entry points for automation and scripting.
- TUI: interactive supervision interface for local development.
- Orchestrator: agent lifecycle, task routing, worktree assignment, and status tracking.
- Workspace management: safe creation, inspection, and cleanup of git worktrees.
- Infrastructure adapters: git, filesystem, process execution, and future agent-provider integrations.

## V1 Boundaries

RaptorCode V1 should coordinate coding-agent workflows inside existing developer environments. It should not replace the user's shell, terminal emulator, package manager, editor, or git hosting provider.

## v0.1 Minimal Architecture Lock

The v0.1 architecture is intentionally minimal. The active architecture path is:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

Rules before v0.1:

- Provider abstraction may exist, but only DeepSeek execution is implemented before v0.1.
- The TUI is a simple Ratatui dashboard only.
- `raptor swarm` is role-based and worktree-based.
- Agent roles are limited to the v0.1 demo path.
- No plugin architecture before v0.1.
- No policy enforcement expansion before v0.1.
- No additional model provider implementation before v0.1.
- No desktop terminal before v0.1.

The launch UX is part of v0.1. Running plain `raptor` shows a premium Raptor Labs / Raptor Code terminal startup screen before the developer prompt becomes ready. The launch UX detects whether `DEEPSEEK_API_KEY` exists and directs missing-key users to `https://platform.deepseek.com/api_keys` without printing, prompting for persistence, or storing API keys.

## Current Implementation

The repository contains a minimal Cargo workspace with crate boundaries for core metadata, task metadata, CLI, git diagnostics, tooling diagnostics, agent orchestration, providers, TUI, sandboxing, and local storage. The implemented CLI router supports `raptor init`, `raptor provider list`, `raptor agent list`, `raptor agent show <id>`, `raptor agent create`, `raptor agent policy`, `raptor chat`, `raptor task`, `raptor swarm`, `raptor swarm implement`, `raptor review`, `raptor diff`, `raptor test`, `raptor merge`, and `raptor doctor`.

`raptor provider list` reads safe project configuration and reports registered provider metadata without external requests or model calls. Base `raptor` renders the startup screen; interactive terminals receive the styled banner, non-TTY and `CI=true` output receive a minimal plain line, and `NO_COLOR` disables ANSI color. `raptor tui` opens the simple read-only Ratatui dashboard in interactive terminals and prints a plain dashboard snapshot otherwise. `raptor chat "message"` sends a non-streaming DeepSeek chat completion request using `DEEPSEEK_API_KEY` and prints the assistant response without executing it or writing files. `raptor agent create` persists local read-only agent session metadata under `.raptor/agents/`; `raptor agent list` and `raptor agent show <id>` inspect that persisted state; `raptor agent policy` displays the deny-by-default permission model. There is no autonomous coding loop, shell execution from model output, patch application path, or automatic application source modification path. `raptor task "description"` validates task input, creates a deterministic isolated git worktree under `.raptor/worktrees/`, and persists task metadata under `.raptor/tasks/`. `raptor swarm "description"` validates task input, creates four role branches and four isolated role worktrees under `.raptor/worktrees/<task-id>/`, persists swarm metadata under `.raptor/swarms/<task-id>.toml`, and, when `DEEPSEEK_API_KEY` is set, calls DeepSeek once per role to write `RAPTOR_PLAN.md` planning artifacts inside each role worktree. If the key is missing, swarm worktree creation still succeeds and metadata records `blocked_missing_deepseek_api_key`. `raptor swarm implement <task-id> <role>` validates one selected role, reads that role's plan, calls DeepSeek once, and writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside that role worktree. `raptor task list` and `raptor task show <id>` read persisted single-task metadata without modifying files. `raptor task abandon <id>` records abandoned lifecycle state without deleting metadata or worktrees. `raptor task cleanup <id>` validates the stored worktree path and removes only the associated worktree directory after exact typed confirmation. `raptor review <task-id>` and `raptor diff <task-id>` validate stored single-task worktree paths when task metadata exists; otherwise they load swarm metadata and inspect all role worktrees read-only. `raptor review <task-id> <role>` and `raptor diff <task-id> <role>` inspect one validated swarm role worktree. `raptor test <task-id>` validates task or swarm role worktree paths, detects `cargo test`, `npm test`, or `make test` when possible, accepts explicit test commands after `--`, and runs them with structured process arguments inside the validated worktree. `raptor merge <task-id>` performs preflight dry-run checks only. `raptor merge <task-id> --execute` requires typed confirmation, performs supervised local merge execution for committed task branch changes, and records lifecycle state as `merged` or `merge_conflict`. The task and swarm workflows do not launch autonomous coding agents. `raptor init` and `raptor doctor` also perform meaningful work today.

## Swarm MVP Boundary

The v0.1 swarm implementation proposal MVP is deliberately limited to metadata, worktree orchestration, planning artifacts, and proposal artifacts.

- `raptor swarm "description"` creates one deterministic task id.
- Role constants are `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- Branches use `raptor/<task-id>/<role>`.
- Worktrees use `.raptor/worktrees/<task-id>/<role>`.
- Swarm metadata uses `.raptor/swarms/<task-id>.toml`.
- Metadata records task description, status, planning status, provider `deepseek`, model `deepseek-chat`, role ids, branches, worktree paths, per-role planning status, per-role plan file paths, per-role implementation status, and per-role proposal file paths.
- Role templates live in the repository root `templates/` directory.
- Swarm planning sends only the task description and role template to DeepSeek.
- Generated plans are written as `RAPTOR_PLAN.md` in each assigned role worktree.
- `raptor swarm implement <task-id> <role>` sends only the task description, selected role, and selected role plan to DeepSeek.
- Generated implementation proposals are written as `RAPTOR_IMPLEMENTATION_PROPOSAL.md` in the selected role worktree.
- Automatic source file modifications inside role worktrees are still out of scope except for `RAPTOR_PLAN.md` and `RAPTOR_IMPLEMENTATION_PROPOSAL.md` artifacts.
- Model output is not executed, applied as patches, or used to run commands.
- `raptor review <task-id>` and `raptor diff <task-id>` fan out across all swarm role worktrees when swarm metadata exists and no single-task metadata exists.
- `raptor review <task-id> <role>` and `raptor diff <task-id> <role>` inspect one selected swarm role.
- Review and diff are read-only and do not write files, update metadata, stage changes, commit, merge, push, run tests, or execute model output.
- `raptor test <task-id>` runs one detected or explicit test command in each validated role worktree for swarm tasks.
- Test execution does not install dependencies, use a shell, call AI models, execute model output, update metadata, commit, merge, or push.
- Persisted agent sessions and permission policy remain frozen and are not expanded by the swarm MVP.

## Provider Boundary

`raptor-providers` owns provider and model metadata in the current foundation. For v0.1, it must grow only enough to execute DeepSeek.

- Built-in provider catalog lists DeepSeek first. Other provider boundaries may remain as future metadata but must not receive execution before v0.1.
- DeepSeek execution uses `POST https://api.deepseek.com/chat/completions`.
- `raptor chat "message"` reads `DEEPSEEK_API_KEY` from the environment and does not read or store API keys from repository files.
- The request sends only a fixed RaptorCode system prompt and the user-provided chat message.
- `raptor swarm "description"` sends only a fixed planning system prompt, the task description, and the selected role template.
- `raptor swarm implement <task-id> <role>` sends only a fixed proposal system prompt, task description, selected role, and selected role `RAPTOR_PLAN.md`.
- Provider ids and model ids are validated before use.
- Provider capabilities describe chat, streaming, system prompt, tool call, local, and network requirements.
- Completion request and response types include assistant content, finish reason, and token usage when DeepSeek returns it.
- Non-200 responses, authentication failures, rate limits, transport failures, and malformed responses return explicit errors without printing API keys.
- No API keys or secrets are stored in config. No other provider receives execution before v0.1.

`raptor-store` parses provider configuration from `raptor.toml` using the existing local config boundary. New project configs include:

```toml
[providers.deepseek]
enabled = true
```

Provider configuration currently records enablement only. It does not contain API keys or secrets.

## Agent Session Boundary

`raptor-agent` owns local agent session state and permission policy models in this foundation.

- Agent ids are validated before lookup.
- Sessions model role, lifecycle state, execution mode, optional task binding, optional provider/model metadata, timestamps, messages, events, audit summaries, and audit entries.
- The default execution mode is `read_only`.
- Read-only mode does not allow file modification, shell execution, or model calls.
- Session metadata is persisted under `.raptor/agents/<agent-id>.toml`.
- Agent creation is metadata-only and refuses duplicate deterministic session ids.
- `raptor agent policy` exposes the current deny-by-default policy: manual approval, repository-root-only file-read modeling, denied file write, denied shell execution, denied network, denied model execution, and denied dangerous commands.

No agent session is executed yet. Persisted sessions are local runtime metadata only; they do not trigger model calls, shell commands, network requests, source file writes, or autonomous loops.

Persisted agent sessions and permission policy were implemented early in commit `483135d7e8e8d25de726874eaddfaa7338f77fe0`. They are frozen until after v0.1 and must not expand into advanced memory, policy enforcement hooks, or an enterprise policy engine before launch.

## Task Workflow Boundaries

- `raptor-core`: owns task identifiers, task status, swarm role constants, task metadata, and worktree information models.
- `raptor-cli`: routes CLI commands and coordinates task setup, swarm setup, DeepSeek planning, supervised implementation proposal generation, role-aware review/diff inspection, supervised test execution, lifecycle, merge, and cleanup workflows.
- `raptor-git`: inspects repository roots, validates worktree and branch names, creates single-task and swarm role git worktrees, performs read-only worktree review and diff inspection, executes supervised local merges, and removes validated task worktree directories.
- `raptor-store`: persists local RaptorCode config, reads provider configuration, reads or writes task metadata, writes and updates swarm metadata, records lifecycle status transitions using the existing `.raptor/tasks/*.toml` format, and persists agent session metadata under `.raptor/agents/*.toml`.
- `raptor-providers`: owns provider ids, model ids, capability metadata, completion request/response shapes, and the disabled provider trait boundary.
- `raptor-agent`: owns read-only local session state, lifecycle models, task binding metadata, messages, events, audit entry structures, the permission policy foundation, and v0.1 role planning prompt construction.
- `raptor-tools`: owns local tooling diagnostics plus safe test command detection, validation, execution, and captured output structures.
- `raptor-tui`: owns centralized brand color tokens, startup banner rendering, dashboard state rendering, and the simple read-only Ratatui dashboard loop.

The current task and swarm workflows are intentionally local and supervised. Task creation, swarm creation, proposal generation, inspection, and test execution prepare or inspect isolated work but do not start background agents, modify application source files automatically, commit changes, or push to remotes. The swarm planning path writes only `RAPTOR_PLAN.md` files, and the implementation proposal path writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md`. Review and diff inspect task or swarm role worktrees read-only. Test execution runs only detected or developer-supplied test commands inside validated worktrees. Model output is treated as untrusted text for developer review. The current provider execution paths are limited to `raptor chat "message"`, planning-only `raptor swarm "description"`, and proposal-only `raptor swarm implement <task-id> <role>` with DeepSeek. They do not feed model output into tools, shell commands, or source-file patch application. The current agent foundation is persistence plus frozen policy modeling only. Merge execution is available only through the explicit `--execute` path after preflight and exact typed confirmation.

## Merge Boundary

`raptor merge <task-id>` currently implements preflight dry-run checks only. `raptor merge <task-id> --execute` reuses those checks, prints `MERGE <task-id>`, reads typed confirmation from stdin, performs a local git merge from the task branch into the target branch, and updates task lifecycle status. The merge boundary remains split into preflight, confirmation, execution, and metadata status update phases.

- `raptor-cli`: parses `raptor merge <task-id>` and `raptor merge <task-id> --execute`, coordinates preflight output, and requires typed confirmation before merge execution can continue.
- `raptor-store`: loads existing task metadata from `.raptor/tasks/<task-id>.toml` and records `merged` or `merge_conflict` status transitions.
- `raptor-git`: validates the stored worktree path, inspects main repository cleanliness, inspects task worktree changes, detects unresolved conflicts, and performs the final local merge only after confirmation.
- `raptor-core`: owns task ids and task status concepts used by creation, merge status updates, abandonment, and cleanup safety checks.

The first merge version must remain local and supervised. It must not auto-merge, force merge, auto-resolve conflicts, delete worktrees, push remotes, run AI agents, or install dependencies.

## Cleanup Boundary

`raptor task cleanup <task-id>` is intentionally narrower than repository cleanup or branch cleanup.

- `raptor-cli`: prints the cleanup plan, requires exact `CLEANUP <task-id>` confirmation, and reports what was retained.
- `raptor-store`: loads existing task metadata; cleanup does not delete metadata in V1.
- `raptor-git`: validates the stored worktree path under `.raptor/worktrees/<task-id>` and deletes only that directory through filesystem APIs.

Cleanup refuses unsafe task ids, unsafe stored paths, missing worktrees, and statuses other than `merged`, `completed`, or `abandoned`. It does not delete branches, push remotes, run tests, execute task metadata commands, or call AI models.
