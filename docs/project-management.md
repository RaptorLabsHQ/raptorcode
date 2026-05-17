# Project Management

Date: 2026-05-17
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
- Persisted agent session metadata.
- Permission policy foundation.
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

The repository has project documentation, governance files, a minimal Rust workspace, a CLI command router, `raptor init`, `raptor doctor`, provider metadata inspection, persisted read-only agent session metadata, a deny-by-default permission policy display, and a first safe task workflow. The task workflow validates input, creates an isolated worktree, persists local task metadata, supports read-only metadata and worktree inspection through `raptor task list`, `raptor task show <id>`, `raptor review <task-id>`, and `raptor diff <task-id>`, includes merge dry-run plus supervised local merge execution, records lifecycle status updates, and supports safe cleanup for merged, completed, or abandoned task worktrees without launching autonomous agents.

Provider and agent foundations are metadata-only. `raptor agent create` creates `.raptor/agents/<agent-id>.toml` records without model calls, shell execution, source file writes, or network requests. V1 TUI and full multi-agent orchestration are not complete.

## Near-Term Backlog

- Add cleanup status recording after validated worktree deletion.
- Add cleanup behavior for failed task creation before task metadata is available.
- Add TUI-visible lifecycle, agent, policy, and worktree cleanup state.
- Add policy enforcement hooks before any future model or tool execution.
- Design the first explicit model invocation command while keeping autonomous repository modification disabled.
