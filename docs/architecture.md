# Architecture

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Architecture Goals

- Keep local developer supervision central to all agent workflows.
- Isolate agent work by default using git worktrees.
- Make repository state, agent state, and pending actions visible.
- Require explicit approval for commits, merges, pushes, and destructive filesystem operations.
- Prefer minimal Rust modules with clear boundaries.

## Planned Layers

- CLI: command entry points for automation and scripting.
- TUI: interactive supervision interface for local development.
- Orchestrator: agent lifecycle, task routing, worktree assignment, and status tracking.
- Workspace management: safe creation, inspection, and cleanup of git worktrees.
- Infrastructure adapters: git, filesystem, process execution, and future agent-provider integrations.

## V1 Boundaries

RaptorCode V1 should coordinate coding-agent workflows inside existing developer environments. It should not replace the user's shell, terminal emulator, package manager, editor, or git hosting provider.

## Current Implementation

The repository contains a minimal Cargo workspace with crate boundaries for core metadata, CLI, git diagnostics, tooling diagnostics, agent orchestration, providers, TUI, sandboxing, and local storage. The implemented CLI surface is limited to `raptor --version` and `raptor doctor`.
