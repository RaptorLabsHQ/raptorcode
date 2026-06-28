# RaptorCode

RaptorCode is a **proprietary, closed-source** Rust-native multi-agent coding terminal from RaptorLabs.dev that runs AI coding workflows in isolated git worktrees with developer supervision.

- Company: RaptorLabs.dev
- Website: https://raptorlabs.dev
- Source repository: private (https://github.com/RaptorLabsHQ/raptor-code — access-controlled)
- License: Proprietary — see [`LICENSE`](LICENSE) (RaptorLabs Proprietary EULA)
- Distribution: official signed binaries for Linux, macOS, and Windows from https://raptorlabs.dev
- Current status: v0.4.0

> RaptorCode is a commercial product, not open source and not free software. The
> source code, system prompts, and orchestration logic are RaptorLabs trade
> secrets. Reverse engineering, decompilation, and redistribution are prohibited
> by the license. Use requires a valid subscription or beta authorization.

RaptorCode is a local-first supervised coding terminal. It is not a desktop app, a Warp clone, a full terminal emulator replacement, an enterprise policy platform, or an unsupervised developer replacement.

## What Works Today

RaptorCode currently focuses on:

- A Rust workspace with the `raptor` CLI binary.
- A premium Raptor Labs / Raptor Code startup screen when running `raptor`.
- A simple read-only Ratatui dashboard through `raptor tui`.
- DeepSeek-first chat through `raptor chat`.
- Local task metadata under `.raptor/`.
- Isolated git worktrees for single tasks and swarm roles.
- Four swarm roles: `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- DeepSeek-generated role plans written as `RAPTOR_PLAN.md`.
- DeepSeek-generated supervised implementation proposals written as `RAPTOR_IMPLEMENTATION_PROPOSAL.md`.
- A live four-specialist agent swarm through `raptor swarm run "mission"` (or `/swarm` in the interactive REPL) implementing in parallel isolated role worktrees under a real-time dashboard, with all changes left uncommitted for review.
- Role-aware review and diff inspection.
- Supervised test execution inside validated worktrees.
- Supervised merge preflight and explicit local merge execution.
- Read-only provider, agent, policy, and diagnostic inspection commands.

DeepSeek is the only implemented execution provider today. Other provider names may appear only as future metadata boundaries; OpenAI, OpenRouter, Gemini, Ollama, LM Studio, MCP, cloud, desktop, team, marketplace, and enterprise systems are not yet implemented.

## Install (Official Binaries)

The supported way to install RaptorCode is the official signed binary for your
platform from https://raptorlabs.dev (Linux, macOS, and Windows). A valid
subscription or beta authorization is required.

On Debian/Ubuntu, install the official `.deb`:

```bash
sudo apt install ./raptor-code_0.4.0_amd64.deb
```

Verify the installed commands:

```bash
raptor --help
raptorcode --help
```

Build the Debian package locally from the repository root:

```bash
./scripts/package-deb.sh
```

The package is written to `dist/`.

## Build From Source (Authorized RaptorLabs Developers Only)

The source repository is private and access-controlled. Building from source is
restricted to authorized RaptorLabs developers under the proprietary license;
it is not a public distribution channel.

Prerequisites:

- Linux, macOS, or another Rust-supported development environment.
- Git, with authorized access to the private repository.
- Stable Rust toolchain and Cargo.

Clone and install the `raptor` command:

```bash
git clone git@github.com:RaptorLabsHQ/raptor-code.git
cd raptor-code
cargo install --path crates/raptor-cli --force
```

Verify the installed command:

```bash
raptor --help
raptor
raptor doctor
raptor tui
```

Run verification locally:

```bash
cargo fmt
cargo clippy --all-targets --all-features
cargo test
```

## DeepSeek Setup

RaptorCode reads the DeepSeek API key from the environment:

```bash
export DEEPSEEK_API_KEY="..."
```

Get a key from:

```text
https://platform.deepseek.com/api_keys
```

RaptorCode does not print, store, commit, or read API keys from `.env` files. If the key is missing, DeepSeek-dependent commands report the missing key and keep local metadata/worktrees reviewable where practical.

## Quickstart

Show the startup screen:

```bash
raptor
```

Inspect the local environment:

```bash
raptor --help
raptor doctor
raptor provider list
raptor agent policy
```

Run DeepSeek chat after setting `DEEPSEEK_API_KEY`:

```bash
raptor chat "Explain this project"
```

Create a supervised multi-agent swarm:

```bash
raptor swarm "Add GitHub OAuth to this Next.js app"
```

Generate an implementation proposal for one role:

```bash
raptor swarm implement <task-id> frontend-agent
```

Run the live four-specialist agent swarm with the real-time dashboard:

```bash
raptor swarm run "Build a SaaS landing page with auth"
```

Review, diff, and test the work:

```bash
raptor review <task-id>
raptor diff <task-id>
raptor test <task-id>
```

Open the read-only dashboard:

```bash
raptor tui
```

Development from source:

The package is internally named `raptor-cli`, but the installed binary and user command are `raptor`.

```bash
cargo run -p raptor-cli --bin raptor -- --help
cargo run -p raptor-cli --bin raptor -- doctor
```

Installed binary commands:

```bash
raptor
raptor --help
raptor doctor
raptor provider list
raptor agent policy
raptor chat "Explain this project"
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor swarm implement <task-id> frontend-agent
raptor swarm run "Build a SaaS landing page with auth"
raptor review <task-id>
raptor diff <task-id>
raptor test <task-id>
raptor tui
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

Current controls:

- Work happens in isolated git worktrees under `.raptor/worktrees/`.
- Task metadata is stored under `.raptor/tasks/`.
- Swarm metadata is stored under `.raptor/swarms/`.
- Agent metadata is stored under `.raptor/agents/`.
- Task ids, role names, branch names, and paths are validated.
- Path traversal is rejected.
- Git and test commands use structured process arguments instead of shell interpolation.
- `raptor chat` does not send repository contents, `.env` files, secrets, task metadata, or worktree files.
- `raptor swarm` sends only the task description and role prompt to DeepSeek.
- `raptor swarm` writes only `RAPTOR_PLAN.md` planning artifacts in role worktrees.
- `raptor swarm implement` writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` in the selected role worktree.
- Model output is never executed in the planning (`raptor swarm`) and proposal (`raptor swarm implement`) paths.
- Patches are not applied automatically in those paths.
- `raptor swarm run` and consent-gated interactive agentic mode execute model-requested tools under the interactive tool policy, inside isolated role worktrees for the swarm, with all changes left uncommitted for developer review.
- The dashboard is read-only and performs no model calls, shell execution, metadata writes, commits, merges, or pushes.
- Merge execution requires explicit `--execute` and exact typed confirmation.
- Advanced policy enforcement hooks beyond the current interactive tool policy are not yet implemented.

Developers remain responsible for reviewing plans, proposals, diffs, tests, and merges.

## Current Limitations

RaptorCode does not yet include:

- Fully autonomous coding loops.
- Automatic patch application.
- OpenAI, OpenRouter, Gemini, Ollama, or LM Studio execution providers.
- MCP.
- Desktop UI.
- Cloud sync.
- Team accounts.
- Marketplace features.
- Enterprise policy engine.
- Production compliance certification.
- Secure hosted sandboxing guarantees.

Persisted agent sessions and permission policy modeling exist as early local metadata foundations, but they are frozen and are not yet an active policy enforcement system.

## Roadmap

Future phases can evaluate:

- Deeper supervised implementation workflows.
- Stronger review, test, and merge ergonomics.
- Provider expansion only after DeepSeek-first workflows are stable.
- More capable TUI surfaces.
- Policy and permission work only after BOB explicitly reopens that scope.

Phase 2 work is intentionally separated from the current release path.

## Repository Layout

```text
.
|-- crates/
|   |-- raptor-agent/
|   |-- raptor-cli/
|   |-- raptor-core/
|   |-- raptor-git/
|   |-- raptor-providers/
|   |-- raptor-sandbox/
|   |-- raptor-store/
|   |-- raptor-tools/
|   `-- raptor-tui/
|-- docs/
|   |-- architecture.md
|   |-- onboarding.md
|   |-- project-management.md
|   |-- releases.md
|   |-- security.md
|   |-- task-workflow.md
|   `-- tech-stack.md
|-- AGENTS.md
|-- CHANGELOG.md
|-- CONTRIBUTING.md
|-- LICENSE
|-- ROADMAP.md
|-- SECURITY.md
`-- Cargo.toml
```

`.raptor/` is ignored by git and reserved for local runtime state.

## Documentation

- [**Persistent Context Handover — RaptorCode's Memory System**](docs/persistent-context-handover.md) ⬅ unique capability
- [Architecture](docs/architecture.md)
- [Security Design](docs/security.md)
- [Task Workflow](docs/task-workflow.md)
- [Tech Stack](docs/tech-stack.md)
- [Onboarding](docs/onboarding.md)
- [Project Management](docs/project-management.md)
- [Release Process](docs/releases.md)
- [Roadmap](ROADMAP.md)
- [Changelog](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)
- [Security Policy](SECURITY.md)

## License

RaptorCode is proprietary, closed-source software. © 2026 RaptorLabs.dev. All
rights reserved. Use is governed by the RaptorLabs Proprietary EULA in
[`LICENSE`](LICENSE). Reverse engineering, decompilation, redistribution, and
derivative works are prohibited.

Third-party open-source components bundled in the binary remain under their own
permissive licenses; see [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md).

Releases up to and including v0.2.0 were published under `MIT OR Apache-2.0`;
that applies only to those prior releases. All releases after v0.2.0 are under
the proprietary license above.
