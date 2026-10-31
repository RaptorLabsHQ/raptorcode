# RaptorCode Roadmap

Date: 2026-08-16
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs AI coding agents in isolated git worktrees with developer supervision. It is local-first: the developer runs it on their own machine and supplies their own model API key.

For the detailed, per-release history see [`CHANGELOG.md`](CHANGELOG.md). This file tracks the current state and forward direction only.

## Current status — v0.6.5, generally available

RaptorCode is generally available on **Linux, macOS, and Windows**. The full test suite is green on all three platforms in CI (Linux, macOS, and Windows are blocking gates). The product is no longer positioned as a closed beta.

Shipped and stable (see the changelog for specifics):

- `raptor` / `raptorcode` CLI: `init`, `doctor`, `chat`, `task`, `swarm`, `review`, `diff`, `test`, `merge`, `agent`, `provider`, `tui`.
- The interactive agentic REPL with per-session file-system consent and six local tools (`write_file`, `read_file`, `list_directory`, `create_directory`, `change_directory`, `run_command`).
- The **live multi-agent swarm**: four specialist agents (frontend, backend, test, security), each in its own git worktree, driven behind a ratatui dashboard.
- DeepSeek provider execution (default model `deepseek-v4-pro`), with retry/backoff, split connect/request timeouts, and streaming.
- Crash-safe atomic local state under `.raptor/`; conservative argv-based git; supervised merge preflight/execution.
- Security posture: deny-by-default tool policy, network-egress confirmation, symlink-safe write boundary, and a hardened shell deny-list backstop (see [`docs/security.md`](docs/security.md)).
- Automated versioning + multi-platform release CD; `cargo fmt` / `clippy -D warnings` / `cargo test` / `cargo audit` CI gates.

## Product posture

- **Local-trust, no hosting.** The agent runs on the developer's own machine under explicit consent, with the developer's own API key. OS-level isolation (namespaces/seccomp/landlock in the `raptor-sandbox` crate) is intentionally **out of scope** at this posture; the userspace tool policy is a proportionate backstop.
- **DeepSeek-first.** DeepSeek V4 Pro is the current target model.

## Forward direction (candidates, not commitments)

Near-term engineering:

- Continue hardening the tool policy backstops (deny-by-default is in; keep the deny-list and egress gating current).
- Keep the cross-platform test suite green as features land.

Exploratory (optional):

- **Additional providers** — accept keys for Anthropic, OpenAI, Google Gemini, and Grok/xAI. The `Provider` trait and `ProviderCatalog` already scaffold this; the work is implementing the trait per provider, removing the hard-coded DeepSeek gates, and parameterizing the DeepSeek-specific assumptions (reasoning-content echo, the context-window constant).
- Persisted/restorable agent sessions and richer multi-file context injection.

## Non-goals

- Full terminal-emulator replacement.
- Hosted / multi-tenant execution.
- Enterprise policy engine, plugin marketplace, or team control plane (not before the above lands).

## Release process

Versioning and releases are automated: functional (`feat`/`fix`/`perf`/`refactor`) pushes to `main` cut a semver bump, tag, and multi-platform GitHub release. Never hand-roll versions or tags. See `scripts/release.sh` and the `auto-release.yml` / `release.yml` workflows.
