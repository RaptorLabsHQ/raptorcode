# Project Management

Date: 2026-05-16
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
- Provider abstraction foundation.
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

The repository has project documentation, governance files, a minimal Rust workspace, a CLI command router, `raptor init`, `raptor doctor`, provider metadata inspection, read-only agent session inspection, and a first safe task workflow. The task workflow validates input, creates an isolated worktree, persists local task metadata, supports read-only metadata and worktree inspection through `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, and `raptor diff <task-id>`, includes merge dry-run plus supervised local merge execution, records lifecycle status updates, and supports safe cleanup for merged, completed, or abandoned task worktrees without launching autonomous agents.

Provider and agent foundations are metadata-only. V1 TUI and full multi-agent orchestration are not complete.

## Near-Term Backlog

- Add cleanup status recording after validated worktree deletion.
- Add cleanup behavior for failed task creation before task metadata is available.
- Add first persisted agent session store when execution policy is designed.
- Add TUI-visible lifecycle and worktree cleanup state.
