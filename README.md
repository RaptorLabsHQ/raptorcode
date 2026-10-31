# RaptorCode

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs
AI coding agents in isolated git worktrees with developer supervision. It is
local-first: it runs on your own machine, with your own model API key, and
every change an agent makes stays uncommitted in a worktree until you review,
test, and merge it.

- License: `MIT OR Apache-2.0` — see [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE)
- Repository: https://github.com/RaptorLabsHQ/raptorcode
- Website and prebuilt binaries: https://raptorlabs.dev
- Current release: v0.6.5 — Linux, macOS, and Windows

RaptorCode is not a desktop app, a terminal emulator replacement, an enterprise
policy platform, or an unsupervised developer replacement.

## What It Does

- `raptor` opens an interactive agentic REPL: after a per-session file-system
  consent prompt the agent inspects the project, plans, edits files, and runs
  commands through a deny-by-default tool policy with developer confirmations.
- `raptor swarm run "mission"` (or `/swarm` inside the REPL) launches a live
  four-specialist swarm — frontend, backend, test, and security agents — each
  working in its own isolated git worktree behind a real-time dashboard.
- `raptor swarm "task"` and `raptor swarm implement <task-id> <role>` produce
  supervised role plans (`RAPTOR_PLAN.md`) and implementation proposals
  (`RAPTOR_IMPLEMENTATION_PROPOSAL.md`) without touching your tree.
- `raptor review`, `raptor diff`, `raptor test`, and `raptor merge` inspect,
  test, and merge worktree results with explicit confirmation.
- `raptor chat` for plain model chat, `raptor tui` for a read-only dashboard,
  `raptor doctor` for environment diagnostics.

DeepSeek is the implemented execution provider today (default model
`deepseek-v4-pro`). The `Provider` trait and provider catalog are the extension
point for others; see [`ROADMAP.md`](ROADMAP.md).

## Install

Prebuilt binaries for Linux (`.deb`), macOS (universal), and Windows are
published on https://raptorlabs.dev with SHA-256 checksums. On Debian/Ubuntu:

```bash
sudo apt install ./raptor-code_0.6.5_amd64.deb
raptor --help
```

### From source

Prerequisites: Git, and Rust 1.88 or newer with Cargo (the repository pins
1.95.0 through `rust-toolchain.toml`).

```bash
git clone https://github.com/RaptorLabsHQ/raptorcode.git
cd raptorcode
cargo install --path crates/raptor-cli --force --locked
```

The package is named `raptor-cli`; the installed commands are `raptor` and
`raptorcode`. Build the Debian package locally with `./scripts/package-deb.sh`
(written to `dist/`).

## DeepSeek Setup

Get an API key from https://platform.deepseek.com/api_keys. RaptorCode reads
`DEEPSEEK_API_KEY` from the environment, or asks for the key once on first run
and stores it in `~/.raptor/.credentials` (owner-only permissions). The key is
never printed, logged, or committed, and `/api` inside the REPL manages it
without echoing it.

## Quickstart

```bash
raptor                      # startup screen, then the interactive REPL
raptor doctor               # check the local environment
raptor chat "Explain this project"
raptor swarm run "Build a SaaS landing page with auth"
raptor review <task-id>     # inspect what the swarm produced
raptor diff <task-id>
raptor test <task-id>
raptor merge <task-id> --execute
raptor tui                  # read-only dashboard
```

From a source checkout, the same commands run through Cargo:

```bash
cargo run -p raptor-cli --bin raptor -- --help
cargo run -p raptor-cli --bin raptor -- doctor
```

## Command Surface

```text
raptor
raptor --version
raptor init
raptor tui
raptor provider list
raptor agent list
raptor agent show <id>
raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]
raptor agent policy
raptor chat "message"
raptor task "task description"
raptor task list
raptor task show <id>
raptor task abandon <id>
raptor task cleanup <id>
raptor swarm "task description"
raptor swarm implement <task-id> <role>
raptor swarm run "mission description"
raptor review <task-id> [role]
raptor diff <task-id> [role]
raptor test <task-id> [-- <command>]
raptor merge <task-id>
raptor merge <task-id> --execute
raptor doctor
```

## Safety Model

RaptorCode treats model output and user-provided identifiers as untrusted.

- Agent work happens in isolated git worktrees under `.raptor/worktrees/`;
  task, swarm, and agent metadata live under `.raptor/tasks/`, `.raptor/swarms/`,
  and `.raptor/agents/`. `.raptor/` is git-ignored.
- Task ids, role names, branch names, and paths are validated; path traversal is
  rejected and writes stay inside the worktree boundary (symlink-safe).
- Git and test commands use structured process arguments, never shell
  interpolation.
- Tool execution runs through a deny-by-default policy: catastrophic commands
  are hard-denied, privileged or network-egress commands and writes outside the
  worktree ask for confirmation, and shell wrappers cannot hide a denied
  command.
- The planning (`raptor swarm`) and proposal (`raptor swarm implement`) paths
  never execute model output or apply patches.
- `raptor chat` sends only your message; it does not send repository contents,
  `.env` files, secrets, task metadata, or worktree files.
- The dashboard is read-only. Merging requires `--execute` and an exact typed
  confirmation.

Developers remain responsible for reviewing plans, proposals, diffs, tests, and
merges. The full design is in [`docs/security.md`](docs/security.md).

## Current Limitations

- Automatic patch application and fully autonomous loops outside the consented
  REPL and swarm paths.
- Providers other than DeepSeek, MCP, a desktop UI, cloud sync, team accounts,
  or an enterprise policy engine.
- OS-level sandboxing of the local agent (namespaces, seccomp, landlock); the
  userspace tool policy is a backstop, not a sandbox.

Persisted agent sessions and permission policy modeling exist as early local
metadata foundations and are not yet an active policy enforcement system.

## Repository Layout

```text
.
|-- crates/
|   |-- raptor-agent/        frozen agent session model (see ROADMAP.md)
|   |-- raptor-cli/          the `raptor` binary: REPL, commands, swarm orchestration
|   |-- raptor-core/         shared types, validators, errors
|   |-- raptor-git/          argv-based git operations and worktrees
|   |-- raptor-providers/    provider trait, catalog, DeepSeek client
|   |-- raptor-sandbox/      reserved (empty)
|   |-- raptor-store/        atomic local state under .raptor/
|   |-- raptor-tools/        agent tool implementations
|   `-- raptor-tui/          ratatui dashboard and startup screen
|-- docs/
|-- packaging/               AppStream metainfo for the .deb
|-- scripts/                 release.sh, package-deb.sh
|-- templates/               swarm role templates
|-- CHANGELOG.md
|-- CONTRIBUTING.md
|-- LICENSE-APACHE
|-- LICENSE-MIT
|-- ROADMAP.md
|-- SECURITY.md
|-- THIRD-PARTY-NOTICES.md
`-- Cargo.toml
```

## Documentation

- [Architecture](docs/architecture.md)
- [Security Design](docs/security.md)
- [Task Workflow](docs/task-workflow.md)
- [Tech Stack](docs/tech-stack.md)
- [Onboarding](docs/onboarding.md)
- [Project Management](docs/project-management.md)
- [DeepSeek API Notes](docs/deepseek-api.md)
- [Release Process](docs/releases.md)
- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)

## Contributing

Contributions are welcome — see [`CONTRIBUTING.md`](CONTRIBUTING.md) for the
development setup, the CI checks, and the commit conventions that drive the
automated releases. Report vulnerabilities privately as described in
[`SECURITY.md`](SECURITY.md).

## License

RaptorCode is dual-licensed under the MIT license and the Apache License 2.0,
at your option. Copyright © 2026 Bob Vasic.

Releases v0.3.0 through v0.6.5 were published under a proprietary license;
from this repository onward all code is `MIT OR Apache-2.0`. Third-party
components keep their own permissive licenses; see
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in RaptorCode by you shall be dual licensed as above, without any
additional terms or conditions.
