# Architecture

Date: 2026-05-17
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

## Current Implementation

The repository contains a minimal Cargo workspace with crate boundaries for core metadata, task metadata, CLI, git diagnostics, tooling diagnostics, agent orchestration, providers, TUI, sandboxing, and local storage. The implemented CLI router supports `raptor init`, `raptor provider list`, `raptor agent list`, `raptor agent show <id>`, `raptor agent create`, `raptor agent policy`, `raptor chat`, `raptor task`, `raptor swarm`, `raptor review`, `raptor diff`, `raptor test`, `raptor merge`, and `raptor doctor`.

`raptor provider list` reads safe project configuration and reports registered provider metadata without external requests or model calls. `raptor agent create` persists local read-only agent session metadata under `.raptor/agents/`; `raptor agent list` and `raptor agent show <id>` inspect that persisted state; `raptor agent policy` displays the deny-by-default permission model. There is no autonomous coding loop, shell execution, network execution, model execution, or source file modification path. `raptor task "description"` validates task input, creates a deterministic isolated git worktree under `.raptor/worktrees/`, and persists task metadata under `.raptor/tasks/`. `raptor task list` and `raptor task show <id>` read that persisted metadata without modifying files. `raptor task abandon <id>` records abandoned lifecycle state without deleting metadata or worktrees. `raptor task cleanup <id>` validates the stored worktree path and removes only the associated worktree directory after exact typed confirmation. `raptor review <task-id>` and `raptor diff <task-id>` validate stored worktree paths and inspect task worktree status or diffs without modifying files. `raptor merge <task-id>` performs preflight dry-run checks only. `raptor merge <task-id> --execute` requires typed confirmation, performs supervised local merge execution for committed task branch changes, and records lifecycle state as `merged` or `merge_conflict`. The task workflow does not launch autonomous coding agents. `raptor init` and `raptor doctor` also perform meaningful work today. Other routed commands are explicit stubs until the corresponding V1 workflows are implemented.

## Provider Boundary

`raptor-providers` owns provider and model metadata only in this foundation.

- Built-in provider catalog lists DeepSeek first, followed by future OpenAI/Codex, Claude, Gemini, and local model boundaries.
- Provider ids and model ids are validated before use.
- Provider capabilities describe chat, streaming, system prompt, tool call, local, and network requirements.
- Completion request and response types exist for future integration, but provider execution returns an explicit disabled error today.
- No API keys, secrets, network calls, async runtime, or external service clients are implemented.

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

## Task Workflow Boundaries

- `raptor-core`: owns task identifiers, task status, task metadata, and worktree information models.
- `raptor-cli`: routes CLI commands and coordinates task setup, inspection, lifecycle, merge, and cleanup workflows.
- `raptor-git`: inspects repository roots, validates worktree and branch names, creates git worktrees, performs read-only worktree review and diff inspection, executes supervised local merges, and removes validated task worktree directories.
- `raptor-store`: persists local RaptorCode config, reads provider configuration, reads or writes task metadata, records lifecycle status transitions using the existing `.raptor/tasks/*.toml` format, and persists agent session metadata under `.raptor/agents/*.toml`.
- `raptor-providers`: owns provider ids, model ids, capability metadata, completion request/response shapes, and the disabled provider trait boundary.
- `raptor-agent`: owns read-only local session state, lifecycle models, task binding metadata, messages, events, audit entry structures, and the permission policy foundation.

The current task workflow is intentionally local and supervised. Task creation and inspection prepare isolated work but do not call model providers, start background agents, commit changes, or push to remotes. The current provider foundation is inspection-only, and the current agent foundation is persistence plus policy modeling only. Merge execution is available only through the explicit `--execute` path after preflight and exact typed confirmation.

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
