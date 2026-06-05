# Project Management

Date: 2026-05-18
Author: Bob Vasic
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

- `raptor swarm` MVP.
- DeepSeek provider execution.
- `raptor chat`.
- Agent role templates.
- Minimal worktree file modification path.
- Role-aware swarm review and diff.
- `raptor test`.
- Ratatui dashboard.
- Premium terminal launch screen.
- DeepSeek API key onboarding.
- README install/demo polish.
- Changelog and release process draft.
- Final verification.
- Release notes final review.
- Tag `v0.1.0`.

## v0.1 Scope Lock

No Phase 2 features may be implemented before v0.1 is complete, tagged, documented, and launch-ready.

Explicitly blocked before v0.1:

- No advanced agent memory before v0.1.
- No enterprise policy engine before v0.1.
- No policy enforcement hooks before v0.1.
- No additional providers before v0.1 except DeepSeek.
- No desktop terminal before v0.1.
- No cloud systems, plugin systems, MCP, marketplace features, or team features before v0.1.

Persisted agent sessions and permission policy exist early from commit `483135d7e8e8d25de726874eaddfaa7338f77fe0`, but they are frozen until after v0.1. Keep them if tests pass. Do not expand them on the v0.1 path.

All next implementation must directly support:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

## MVP Recovery Audit

Complete and in-scope for v0.1:

- Rust workspace and `raptor` CLI binary.
- CLI router and required command names.
- `raptor init`.
- `raptor doctor`.
- Safe single-task worktree creation through `raptor task "description"`.
- Task metadata list/show.
- Read-only review and diff inspection.
- Supervised merge preflight and local merge execution.
- `raptor swarm` MVP.
- DeepSeek provider execution through `DEEPSEEK_API_KEY`.
- `raptor chat "message"`.
- Agent role templates.
- Swarm DeepSeek planning artifacts.
- Minimal supervised implementation proposal artifacts.
- Role-aware swarm review and diff inspection.
- Supervised `raptor test <task-id>` execution in validated worktrees.
- Premium startup screen for base `raptor`.
- Simple read-only `raptor tui` Ratatui dashboard.
- Startup DeepSeek API key detection and fallback URL.

Remaining before v0.1 tag:

- Final verification.
- Release notes final review.
- Tag `v0.1.0`.

Implemented early but beyond strict v0.1 scope:

- Persisted `.raptor/agents/<agent-id>.toml` sessions.
- `raptor agent create`.
- Persisted `raptor agent list/show`.
- `raptor agent policy`.
- Deny-by-default permission policy modeling.

Code or docs that were pointing toward Phase 2 before v0.1:

- Previous project-management backlog recommended policy enforcement hooks.
- Previous task-workflow next step recommended cleanup lifecycle refinements instead of swarm.
- Documentation emphasized future providers and policy modeling too prominently for the active v0.1 path.

Direct blockers for v0.1 launch:

- Final verification is not complete.
- Release notes need final review.
- `v0.1.0` tag is not created.

## Definition Of Done

An increment is complete when:

- The requested scope is implemented.
- Relevant documentation is updated.
- Available checks have been run.
- The diff has been reviewed.
- The change is committed and pushed when credentials are available.

## Refactor Delivery Rules

Date: 2026-06-05
Author: Bob Vasic
Company: RaptorLabs.dev

Agile delivery for RaptorCode means small, shippable, verified increments with clear ownership boundaries. Do not treat "microservices" as a default quality goal for this CLI product. The current scalable architecture is a modular Rust workspace; distributed services should be introduced only after a concrete runtime, deployment, or team-boundary requirement exists.

SOLID cleanup priorities for upcoming iterations:

- Extract one responsibility at a time from `raptor-cli/src/lib.rs`.
- Preserve the public CLI contract unless BOB explicitly approves a behavior change.
- Add focused tests before or with each extracted boundary.
- Keep terminal rendering, provider I/O, persistence, git operations, and process execution in separate modules or crates.
- Prefer deleting duplication and narrowing mutable state over adding abstractions.
- Keep each commit coherent enough to review independently.

Current refactor sequence:

- Completed: extract DeepSeek context-budget decisions into `raptor-cli::context_budget`.
- Next: extract interactive local tool definitions and execution into a dedicated CLI module.
- Then: extract interactive slash-command handling from the main REPL loop.
- Then: split provider streaming assembly from synchronous request execution if DeepSeek support continues to expand.

## Current Status

The repository has project documentation, governance files, a minimal Rust workspace, a CLI command router, a branded base `raptor` startup screen, a simple read-only `raptor tui` dashboard, `raptor init`, `raptor doctor`, provider metadata inspection, DeepSeek chat completion through `raptor chat "message"`, persisted read-only agent session metadata, a deny-by-default permission policy display, a safe single-task workflow, and a v0.1 swarm worktree MVP. The task workflow validates input, creates an isolated worktree, persists local task metadata, supports read-only metadata and worktree inspection through `raptor task list`, `raptor task show <id>`, `raptor review <task-id> [role]`, and `raptor diff <task-id> [role]`, runs supervised tests through `raptor test <task-id>`, includes merge dry-run plus supervised local merge execution, records lifecycle status updates, and supports safe cleanup for merged, completed, or abandoned task worktrees without launching autonomous agents. The swarm workflow creates role worktrees, records swarm metadata, can generate DeepSeek plans and implementation proposal artifacts, supports read-only review/diff fan-out or per-role inspection, and runs supervised tests across validated role worktrees.

DeepSeek is the only provider execution path before v0.1. Startup checks only whether `DEEPSEEK_API_KEY` exists and never prints or stores the key. `raptor tui` is read-only and does not call models. `raptor chat` does not execute model output, modify source files, run shell commands, create worktrees, merge, commit, or push. `raptor agent create` creates `.raptor/agents/<agent-id>.toml` records without model calls, shell execution, source file writes, or network requests. Persisted agent sessions and permission policy are frozen until after v0.1. Full multi-agent autonomous orchestration is not complete.

## Near-Term Backlog

- PROMPT 25 - Final verification and tag v0.1.0.
- Run final verification, review release notes, and tag only after BOB explicitly approves tagging.
