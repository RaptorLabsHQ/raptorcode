# Architecture

Date: 2026-05-16
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

The repository contains a minimal Cargo workspace with crate boundaries for core metadata, task metadata, CLI, git diagnostics, tooling diagnostics, agent orchestration, providers, TUI, sandboxing, and local storage. The implemented CLI router supports `raptor init`, `raptor chat`, `raptor task`, `raptor swarm`, `raptor review`, `raptor diff`, `raptor test`, `raptor merge`, and `raptor doctor`.

`raptor task "description"` validates task input, creates a deterministic isolated git worktree under `.raptor/worktrees/`, and persists task metadata under `.raptor/tasks/`. `raptor task list` and `raptor task show <id>` read that persisted metadata without modifying files. `raptor task abandon <id>` records abandoned lifecycle state without deleting metadata or worktrees. `raptor task cleanup <id>` validates the stored worktree path and removes only the associated worktree directory after exact typed confirmation. `raptor review <task-id>` and `raptor diff <task-id>` validate stored worktree paths and inspect task worktree status or diffs without modifying files. `raptor merge <task-id>` performs preflight dry-run checks only. `raptor merge <task-id> --execute` requires typed confirmation, performs supervised local merge execution for committed task branch changes, and records lifecycle state as `merged` or `merge_conflict`. The task workflow does not launch autonomous coding agents. `raptor init` and `raptor doctor` also perform meaningful work today. Other routed commands are explicit stubs until the corresponding V1 workflows are implemented.

## Task Workflow Boundaries

- `raptor-core`: owns task identifiers, task status, task metadata, and worktree information models.
- `raptor-cli`: routes CLI commands and coordinates task setup, inspection, lifecycle, merge, and cleanup workflows.
- `raptor-git`: inspects repository roots, validates worktree and branch names, creates git worktrees, performs read-only worktree review and diff inspection, executes supervised local merges, and removes validated task worktree directories.
- `raptor-store`: persists local RaptorCode config, reads or writes task metadata, and records lifecycle status transitions using the existing `.raptor/tasks/*.toml` format.

The current task workflow is intentionally local and supervised. Task creation and inspection prepare isolated work but do not call model providers, start background agents, commit changes, or push to remotes. Merge execution is available only through the explicit `--execute` path after preflight and exact typed confirmation.

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
