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

## Planned Supervised Merge Workflow

This section defines the intended first `raptor merge <task-id>` workflow. Merge logic is not implemented yet.

### Scope

The first merge workflow must be supervised only.

- `raptor merge <task-id>` must never merge without explicit user confirmation.
- It must never run if the main repository has uncommitted changes unless a future force mode is explicitly designed and documented.
- It must never delete task worktrees in the first merge version.
- It must not push to a remote.
- It must not launch AI agents or run task-provided commands.

### Required Preconditions

The future merge command must stop before any merge attempt unless all of these checks pass:

- The task id is valid and path-safe.
- Task metadata exists under `.raptor/tasks/<task-id>.toml`.
- The metadata contains a worktree path.
- The task worktree path exists.
- The stored worktree path resolves safely under `.raptor/worktrees/<task-id>`.
- The main repository worktree is clean.
- The task worktree has changes to merge.
- No unresolved conflicts are already present in the main repository or task worktree.
- The developer has reviewed `raptor review <task-id>` and/or `raptor diff <task-id>` output before confirming merge.

### Explicit Non-Goals

The first merge version must not include:

- Auto-merge without confirmation.
- Force merge.
- Conflict auto-resolution.
- Worktree deletion.
- Remote push.
- AI agent execution.
- Automatic dependency installation.

### Proposed Command Behavior

`raptor merge <task-id>` should first behave like a dry run:

1. Validate the task id.
2. Load task metadata from `.raptor/tasks/<task-id>.toml`.
3. Validate the stored worktree path against `.raptor/worktrees/<task-id>`.
4. Verify the main repository is clean.
5. Verify the task worktree exists and has changes.
6. Verify no unresolved conflicts are present.
7. Print a dry-run summary with:
   - task id
   - task status
   - task description
   - target branch detected from the main repository
   - source worktree branch from task metadata
   - worktree path
   - changed file list
8. Require typed confirmation exactly matching `MERGE <task-id>`.
9. Abort without changes if confirmation is missing or mismatched.
10. Perform the merge only after successful confirmation.
11. Print final status after success, abort, or conflict.

### Operational Notes

- Clean merge: print the resulting target branch status and leave the task worktree intact.
- Conflict: stop immediately, report conflicted files, do not auto-resolve, do not delete the worktree, and leave the repository state visible for developer recovery.
- Dirty main repository: abort before merge planning and instruct the developer to commit, stash, or discard local changes manually.
- Missing task metadata: return a clear not-found error for the task id.
- Missing task worktree: return a clear worktree-not-found error and do not read arbitrary filesystem paths.
- No task changes: abort because there is nothing to merge.

### Security Notes

- Treat the task id as untrusted input.
- Treat stored task metadata and stored worktree paths as untrusted.
- Reject path traversal and unsafe names.
- Reject worktree paths outside `.raptor/worktrees/<task-id>`.
- Do not execute commands from task descriptions, generated files, or task metadata.
- Do not trust generated files blindly; require developer review before merge confirmation.

### Implementation Plan

The implementation should keep CLI orchestration thin and put git-specific checks in `raptor-git`.

- Extend the CLI parser from the current `raptor merge` stub to `raptor merge <task-id>`.
- Reuse `raptor-store` to load the existing `.raptor/tasks/*.toml` metadata format.
- Reuse and extend `raptor-git` worktree path validation used by `raptor review` and `raptor diff`.
- Add read-only git preflight APIs for main repository cleanliness, task worktree status, changed files, and unresolved conflict detection.
- Add a merge execution API only after dry-run output and typed confirmation are implemented.
- Add tests for precondition failures, confirmation mismatch, clean merge behavior, conflict handling, and no worktree deletion.

### Future Phases

- Cleanup command for completed or abandoned task worktrees.
- Merge conflict assistant that explains conflicts without applying automatic resolution.
- Automated test execution before merge.
- Policy engine for repository-specific merge rules.
- Signed commits.
- Branch protection awareness.

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
- `raptor merge` is still a stub; this document defines intended behavior only.
- No task deletion, cleanup, or archival command exists yet.
- Failed metadata persistence after worktree creation may require manual cleanup.

## Recommended Next Step

Implement `raptor merge <task-id>` preflight checks and dry-run summary without merge execution first.
