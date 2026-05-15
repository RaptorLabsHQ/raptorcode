# Task Workflow

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Summary

The first real workflow is `raptor task "description"`. It prepares safe local task infrastructure only. It does not launch autonomous coding agents.

## Command

```bash
cargo run -p raptor-cli -- task "describe the supervised task"
```

The installed binary form is:

```bash
raptor task "describe the supervised task"
```

## Behavior

The command:

- Requires a non-empty task description.
- Creates a deterministic task id from the description.
- Detects the git repository root.
- Plans a branch named `raptor-task-<task-id>`.
- Creates an isolated git worktree under `.raptor/worktrees/<task-id>`.
- Persists task metadata under `.raptor/tasks/<task-id>.toml`.
- Reports task id, status, branch, worktree path, and metadata path.

## Local State

`.raptor/` is ignored by git and is reserved for local runtime state.

Current layout:

```text
.raptor/
  tasks/
    <task-id>.toml
  worktrees/
    <task-id>/
```

## Safety Rules

- Task descriptions are validated before task ids are created.
- Branch and worktree names must use safe lowercase ASCII, digits, and hyphens.
- Parent-directory traversal is rejected.
- Worktree paths must stay inside the repository root.
- Existing worktree paths are not overwritten.
- Existing task metadata is not overwritten.
- Git commands are invoked through structured process arguments.

## Current Limits

- No autonomous agents are launched.
- No commits are created.
- No branches are merged.
- No remote pushes are performed.
- No task list or task show command exists yet.
- Failed metadata persistence after worktree creation may require manual cleanup.

## Recommended Next Step

Add `raptor task list` and `raptor task show <id>` so developers can inspect task metadata before review and merge workflows are implemented.
