# Security Policy

Date: 2026-08-16
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is a proprietary, closed-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Supported Versions

The latest release is `v0.6.3`, available for Linux, macOS, and Windows. Security reports should target the latest release or the current `main` branch. Only the latest release line is supported.

## Reporting A Vulnerability

Do not disclose exploitable vulnerabilities publicly before maintainers have had time to respond.

Report security issues privately by email to **security@raptorlabs.dev**. Do not open public GitHub issues for exploitable vulnerabilities. Include a description, affected version or commit, and reproduction steps; we aim to acknowledge within a few business days.

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
- Policy enforcement hooks.
- Additional execution providers beyond DeepSeek.
