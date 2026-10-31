# Security Policy

Date: 2026-10-31
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs
AI coding agents safely in isolated git worktrees with developer supervision.
It is local-first: the agent runs on your machine, under your consent, with
your own model API key.

## Supported Versions

Security fixes target the current `main` branch and the latest release
(`v0.6.5` at the time of writing). Only the latest release line is supported.

## Reporting A Vulnerability

Please do not open public GitHub issues for exploitable vulnerabilities.

- Preferred: open a private report through GitHub's
  [private vulnerability reporting](https://github.com/RaptorLabsHQ/raptorcode/security/advisories/new)
  for this repository.
- Alternatively, email **security@raptorlabs.dev**.

Include a description, the affected version or commit, and reproduction steps.
We aim to acknowledge reports within a few business days and to coordinate
disclosure with you once a fix is available.

## Security Priorities

- Isolate agent work in dedicated git worktrees.
- Require developer consent and supervision for tool execution, commits,
  merges, pushes, and destructive actions.
- Validate filesystem paths and keep writes inside the worktree boundary.
- Avoid shell injection and unsafe command construction.
- Keep dependency usage minimal and auditable (`cargo deny`, `cargo audit`,
  and CodeQL run in CI).
- Preserve clear logs for security-relevant operations.

See [`docs/security.md`](docs/security.md) for the full security design.

## Out Of Scope

- Hosted execution infrastructure and remote sandboxing guarantees.
- OS-level isolation (namespaces, seccomp, landlock) of the local agent; the
  userspace tool policy is a backstop, not a sandbox.
- Enterprise identity provider integrations and formal compliance
  certification.
