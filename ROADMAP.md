# RaptorCode Roadmap

Date: 2026-05-16
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

## V1 Scope

V1 scope is CLI + TUI + multi-agent orchestrator.

V1 is not a full terminal emulator replacement. It will focus on supervised coding-agent workflows, not on replacing shells, multiplexers, or general terminal applications.

## Milestones

1. Repository foundation
   - License, contribution, security, and project documentation.
   - Agent operating instructions tracked in the repository.

2. Rust workspace foundation
   - Minimal Cargo workspace.
   - CLI crate skeleton.
   - Shared configuration and error conventions.

3. CLI workflow
   - Commands to initialize project state.
   - Commands to inspect provider metadata and local agent session state.
   - Commands to create, list, inspect, and clean isolated worktrees.
   - Supervised merge preflight for task worktrees.
   - Typed `MERGE <task-id>` confirmation before supervised local merge execution.
   - Task lifecycle status updates for merge success, merge conflict, and abandonment.
   - Typed `CLEANUP <task-id>` confirmation before deleting validated task worktrees.
   - Explicit safety checks before destructive operations.

4. TUI workflow
   - Developer supervision interface.
   - Agent session overview.
   - Worktree status and task state display.

5. Multi-agent orchestrator
   - Provider abstraction with DeepSeek first and future OpenAI/Codex, Claude, Gemini, and local model boundaries.
   - Agent lifecycle management.
   - Read-only agent session state before any autonomous coding loop.
   - Per-agent worktree isolation.
   - Review points before merge, commit, or push actions.

6. Security hardening
   - Command execution boundaries.
   - Path validation.
   - Audit-friendly logs.
   - Dependency review.

## Non-Goals For V1

- Full terminal emulator replacement.
- Unsupervised autonomous production changes.
- Provider API calls before explicit provider integration design.
- Agent shell execution before explicit permissioning design.
- Auto-merge without explicit developer confirmation.
- Force merge and automatic conflict resolution.
- Remote push from the first merge workflow.
- `--no-confirm` merge execution in V1.
- Cloud-hosted control plane.
- Plugin marketplace.
- Cross-repository fleet management.

## Release Criteria

- Safe default behavior for local repositories.
- Clear user confirmation before risky actions.
- Documented architecture and threat model.
- Basic CLI and TUI flows covered by tests.
- Reproducible local build and development instructions.
