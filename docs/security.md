# Security Design

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Security Model

RaptorCode assumes AI coding agents can make incorrect or unsafe changes. The system must keep developers in control and make agent actions inspectable before they affect the main working tree or remote repository.

## Primary Controls

- Worktree isolation for concurrent agent changes.
- Explicit confirmation for destructive operations.
- Clear status reporting before commit, merge, or push operations.
- Path validation for filesystem operations.
- Structured command execution instead of shell string concatenation.
- Minimal dependencies with documented justification.
- Deterministic task identifiers derived from validated task descriptions.
- Local `.raptor/` state ignored by git to avoid accidentally committing runtime metadata or nested worktrees.

## Auditability

Security-relevant actions should be logged or surfaced clearly to the developer:

- Worktree creation and deletion.
- Agent task assignment.
- Command execution requests.
- Commit, merge, and push attempts.
- Failed validation checks.

## Initial Threats

- Path traversal through untrusted task input.
- Shell injection through generated commands.
- Accidental overwrite of user work.
- Unsafely merging unreviewed agent output.
- Leaking secrets through logs or generated files.

## Implemented Task Workflow Controls

`raptor task "description"` currently applies these controls:

- Requires a non-empty task description.
- Generates a deterministic safe task id from the description.
- Validates branch and worktree names before git operations.
- Inspects the git repository root before creating task state.
- Creates worktrees only under `.raptor/worktrees/`.
- Refuses path traversal and unsafe names.
- Refuses to overwrite existing worktree paths.
- Refuses to overwrite existing task metadata.
- Persists task metadata under `.raptor/tasks/`.
- Reads task metadata only from validated `.raptor/tasks/<id>.toml` paths.
- Rejects unsafe task ids and path traversal attempts in task inspection commands.
- Validates stored worktree paths against `.raptor/worktrees/<task-id>` before review or diff inspection.
- Uses read-only git status and diff commands for `raptor review <task-id>` and `raptor diff <task-id>`.
- Runs `raptor merge <task-id>` as a preflight dry run only.
- Refuses merge preflight when the target repository is dirty, the task worktree has no changes, or unresolved conflicts are present.
- Does not launch autonomous agents, merge changes, commit changes, or push remotes.

Known gap: if metadata persistence fails after git worktree creation, manual cleanup may be required until an explicit cleanup workflow is implemented.

## Planned Merge Controls

`raptor merge <task-id>` currently performs preflight checks and prints a dry-run summary only. Merge execution is not implemented yet.

Current preflight controls and future merge execution controls:

- Treat task ids, task metadata, and stored worktree paths as untrusted input.
- Reject path traversal, unsafe task ids, and worktree paths outside `.raptor/worktrees/<task-id>`.
- Refuse to proceed if task metadata or the task worktree is missing.
- Refuse to proceed if the main repository has uncommitted changes.
- Refuse to proceed when unresolved conflicts are already present.
- Require the developer to review `raptor review <task-id>` and/or `raptor diff <task-id>` output before merge confirmation.
- Print a dry-run summary before any mutation.
- Require typed confirmation exactly matching `MERGE <task-id>`.
- Abort on missing or mismatched confirmation.
- Never execute task-provided commands, generated scripts, or dependency installation steps during merge.
- Never push to a remote or delete the task worktree in the first merge version.
- Never trust generated files blindly; the developer remains responsible for code review before confirming merge.

## Current Status

The repository contains a Rust workspace with an implemented CLI router, `raptor init`, `raptor doctor`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, `raptor diff <task-id>`, and `raptor merge <task-id>` preflight dry run. Security controls described here are partly implemented for task setup and inspection, and remain design requirements for future agent, merge execution, and TUI workflows.
