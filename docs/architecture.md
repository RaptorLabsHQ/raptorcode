# Architecture

Date: 2026-05-15
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

`raptor task "description"` validates task input, creates a deterministic isolated git worktree under `.raptor/worktrees/`, and persists task metadata under `.raptor/tasks/`. `raptor task list` and `raptor task show <id>` read that persisted metadata without modifying files. `raptor review <task-id>` and `raptor diff <task-id>` validate stored worktree paths and inspect task worktree status or diffs without modifying files. The task workflow does not launch autonomous coding agents. `raptor init` and `raptor doctor` also perform meaningful work today. Other routed commands are explicit stubs until the corresponding V1 workflows are implemented.

## Task Workflow Boundaries

- `raptor-core`: owns task identifiers, task status, task metadata, and worktree information models.
- `raptor-cli`: routes CLI commands and coordinates task setup and inspection workflows.
- `raptor-git`: inspects repository roots, validates worktree and branch names, creates git worktrees, and performs read-only worktree review and diff inspection.
- `raptor-store`: persists local RaptorCode config and reads or writes task metadata using the existing `.raptor/tasks/*.toml` format.

The current task workflow is intentionally local and supervised. It prepares isolated work but does not call model providers, start background agents, merge branches, commit changes, or push to remotes.

## Planned Merge Boundary

`raptor merge <task-id>` currently implements preflight dry-run checks only. Future merge execution should remain split into preflight, confirmation, and merge execution boundaries.

- `raptor-cli`: parses `raptor merge <task-id>`, coordinates preflight output, and should require typed confirmation before any future merge execution.
- `raptor-store`: load existing task metadata from `.raptor/tasks/<task-id>.toml`; no new metadata format is required for the first merge version.
- `raptor-git`: validates the stored worktree path, inspects main repository cleanliness, inspects task worktree changes, detects unresolved conflicts, and should perform the final merge only after confirmation in a future implementation.
- `raptor-core`: continue owning task ids and task status concepts; merge-specific state should not be added until there is a concrete status transition to persist.

The first merge version must remain local and supervised. It must not auto-merge, force merge, auto-resolve conflicts, delete worktrees, push remotes, run AI agents, or install dependencies.
