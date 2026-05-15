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
cargo run -p raptor-cli -- merge <task-id>
```

Installed binary forms:

```bash
raptor task "describe the supervised task"
raptor task list
raptor task show <id>
raptor review <task-id>
raptor diff <task-id>
raptor merge <task-id>
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

`raptor merge <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Verifies the target repository is a git repository.
- Verifies the target repository worktree is clean.
- Verifies the task worktree exists and has changes.
- Rejects unresolved conflict states.
- Prints a dry-run summary with task metadata, target repository, target branch when detectable, worktree path, worktree branch when detectable, and changed files.
- Explicitly states that no merge was executed.
- Does not stage, commit, merge, push, delete worktrees, run tests, install dependencies, execute task-provided commands, or call AI models.

## Supervised Merge Workflow

This section defines the current dry-run behavior and the intended future merge execution behavior.

### Scope

The first merge workflow must be supervised only.

- `raptor merge <task-id>` must never merge without explicit user confirmation.
- It must never run if the main repository has uncommitted changes unless a future force mode is explicitly designed and documented.
- It must never delete task worktrees in the first merge version.
- It must not push to a remote.
- It must not launch AI agents or run task-provided commands.

### Required Preconditions

The merge command stops before merge execution unless all of these checks pass:

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

### Current Command Behavior

`raptor merge <task-id>` currently performs dry-run preflight only:

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
   - target repository path
   - target branch detected from the main repository
   - worktree branch detected from the task worktree
   - worktree path
   - changed file list
8. Print that no merge was executed.
9. Print the expected future behavior.

Future merge execution must add:

- typed confirmation exactly matching `MERGE <task-id>`
- abort on missing or mismatched confirmation
- merge execution only after successful confirmation
- final status after success, abort, or conflict

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

Implemented preflight pieces:

- CLI parser accepts `raptor merge <task-id>`.
- `raptor-store` loads the existing `.raptor/tasks/*.toml` metadata format.
- `raptor-git` validates stored worktree paths using the same safety boundary as `raptor review` and `raptor diff`.
- Read-only git preflight checks cover main repository cleanliness, task worktree status, changed files, and unresolved conflict detection.

Future merge execution pieces:

- Add typed confirmation before mutation.
- Add merge execution only after dry-run output and confirmation are stable.
- Add tests for confirmation mismatch, clean merge behavior, conflict handling during merge execution, and no worktree deletion.

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
- Merge preflight verifies stored worktree paths before invoking read-only git status inspection.

## Current Limits

- No autonomous agents are launched.
- No commits are created.
- No branches are merged.
- No remote pushes are performed.
- `raptor merge <task-id>` is preflight dry-run only; merge execution is not implemented yet.
- No task deletion, cleanup, or archival command exists yet.
- Failed metadata persistence after worktree creation may require manual cleanup.

## Recommended Next Step

Add typed confirmation and merge execution after dry-run output and safety checks are stable.
