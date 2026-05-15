# Task Workflow

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Summary

The first real workflow is `raptor task "description"`. It prepares safe local task infrastructure only. It does not launch autonomous coding agents.

## Command

Create task infrastructure:

```bash
cargo run -p raptor-cli -- task "describe the supervised task"
```

Inspect persisted task metadata:

```bash
cargo run -p raptor-cli -- task list
cargo run -p raptor-cli -- task show <id>
cargo run -p raptor-cli -- review <task-id>
cargo run -p raptor-cli -- diff <task-id>
```

Installed binary forms:

```bash
raptor task "describe the supervised task"
raptor task list
raptor task show <id>
raptor review <task-id>
raptor diff <task-id>
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

`raptor task list`:

- Reads task metadata from `.raptor/tasks/`.
- Prints task id, status, created timestamp when available, worktree path, and description.
- Handles an empty task directory gracefully.
- Does not modify files.

`raptor task show <id>`:

- Treats the task id as untrusted input.
- Rejects unsafe ids and path traversal attempts.
- Reads only `.raptor/tasks/<id>.toml`.
- Prints full task details.
- Does not modify files.

`raptor review <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Verifies the task worktree exists.
- Prints task id, status, description, worktree path, git status summary, and changed files.
- Does not stage, commit, merge, delete, or modify files.

`raptor diff <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Prints unstaged and staged git diffs for the task worktree.
- Does not stage, commit, merge, delete, or modify files.

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
- Task inspection commands reject unsafe ids and avoid arbitrary file reads.
- Review and diff commands verify stored worktree paths before invoking read-only git inspection.

## Current Limits

- No autonomous agents are launched.
- No commits are created.
- No branches are merged.
- No remote pushes are performed.
- No task deletion, cleanup, or archival command exists yet.
- Failed metadata persistence after worktree creation may require manual cleanup.

## Recommended Next Step

Add supervised merge precondition checks after review and diff inspection output is stable.
