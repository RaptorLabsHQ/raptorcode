# Security Policy

Date: 2026-05-15
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Supported Versions

No stable release exists yet. Security reports should target the current `main` branch until versioned releases begin.

## Reporting A Vulnerability

Do not disclose exploitable vulnerabilities publicly before maintainers have had time to respond.

For now, report security issues through GitHub issues only if the issue does not expose sensitive exploit details. If a private reporting channel is added, this file will be updated.

## Security Priorities

- Isolate agent work in dedicated git worktrees.
- Require developer supervision for commit, merge, push, and destructive actions.
- Validate filesystem paths before use.
- Avoid shell injection and unsafe command construction.
- Keep dependency usage minimal and auditable.
- Preserve clear logs for security-relevant operations.

## Out Of Scope For Early Development

- Hosted execution infrastructure.
- Remote agent sandboxing guarantees.
- Enterprise identity provider integrations.
- Formal compliance certification.
