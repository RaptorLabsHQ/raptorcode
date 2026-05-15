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
- Does not launch autonomous agents, merge changes, commit changes, or push remotes.

Known gap: if metadata persistence fails after git worktree creation, manual cleanup may be required until an explicit cleanup workflow is implemented.

## Current Status

The repository contains a Rust workspace with an implemented CLI router, `raptor init`, `raptor doctor`, and the first safe `raptor task "description"` workflow. Security controls described here are partly implemented for task setup and remain design requirements for future agent, review, merge, and TUI workflows.
