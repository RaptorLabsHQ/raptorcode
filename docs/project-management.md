# Project Management

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Working Model

- Deliver small, coherent increments.
- Keep changes traceable through clear commits.
- Document security or operational assumptions when behavior changes.
- Preserve existing user work.
- Prefer implementation steps that can be verified locally.

## V1 Tracking Areas

- Repository foundation.
- Rust workspace and CLI skeleton.
- CLI command router.
- Safe task workflow foundation.
- Worktree management and inspection.
- Agent orchestration.
- TUI supervision workflow.
- Security hardening.
- Release readiness.

## Definition Of Done

An increment is complete when:

- The requested scope is implemented.
- Relevant documentation is updated.
- Available checks have been run.
- The diff has been reviewed.
- The change is committed and pushed when credentials are available.

## Current Status

The repository has project documentation, governance files, a minimal Rust workspace, a CLI command router, `raptor init`, `raptor doctor`, and a first safe `raptor task "description"` workflow. The task workflow validates input, creates an isolated worktree, and persists local task metadata without launching autonomous agents.

V1 CLI, TUI, review, merge, and multi-agent orchestration are not complete.

## Near-Term Backlog

- Add `raptor task list`.
- Add `raptor task show <id>`.
- Add review inspection for task worktrees.
- Define supervised merge preconditions.
- Add cleanup behavior for failed task creation after worktree creation.
