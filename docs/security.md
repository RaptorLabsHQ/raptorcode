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

## Current Status

This repository currently contains documentation only. Security controls described here are design requirements for the planned Rust implementation.
