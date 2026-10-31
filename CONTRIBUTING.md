# Contributing To RaptorCode

Date: 2026-10-31
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs
AI coding agents safely in isolated git worktrees with developer supervision.
Contributions are welcome: bug reports, fixes, docs, tests, and features that
fit the product posture described in [`ROADMAP.md`](ROADMAP.md).

## Before You Start

- Search the issues first; open one for anything larger than a small fix so the
  approach can be agreed before you invest time.
- Good first areas: packaging (Homebrew, AUR, Nix, Scoop, winget), provider
  implementations behind the existing `Provider` trait, TUI polish, and docs.

## Development Setup

Prerequisites: Git, and Rust 1.88 or newer with Cargo (the repository pins
1.95.0 through `rust-toolchain.toml`, which rustup picks up automatically).

```bash
git clone https://github.com/RaptorLabsHQ/raptorcode.git
cd raptorcode
cargo build --workspace
cargo run -p raptor-cli --bin raptor -- --help
```

Run the same checks CI runs before you open a pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check      # cargo install cargo-deny
cargo audit           # cargo install cargo-audit
```

## Pull Requests

- Keep changes small, reviewable, and focused on one thing.
- Use conventional commit subjects (`feat:`, `fix:`, `perf:`, `refactor:`,
  `docs:`, `test:`, `ci:`, `chore:`). Releases are cut automatically from
  `feat`/`fix`/`perf`/`refactor` commits that land on `main`, so the subject
  decides whether a release happens. Never hand-roll versions or tags.
- Add or update tests for behavior changes; keep the suite green on Linux,
  macOS, and Windows (all three are blocking CI gates).
- Update the relevant documentation under `docs/` and add a line to the
  `Unreleased` section of `CHANGELOG.md`.
- Preserve developer control over commit, merge, push, and destructive actions:
  nothing the agent does may bypass the consent prompts, the tool policy, or the
  supervised merge path.

## Code Standards

- Safe Rust only; `unsafe` needs a written justification in the PR.
- Explicit error handling with `Result` and typed errors; `unwrap` is denied by
  the workspace lints.
- Treat model output and user-provided identifiers as untrusted input.
- Avoid new dependencies unless they clearly reduce risk or complexity, keep
  them permissively licensed (MIT, Apache-2.0, BSD, ISC, or equivalent;
  `cargo deny check` enforces the allow-list), and record direct dependencies
  in `THIRD-PARTY-NOTICES.md`.
- Never commit secrets, tokens, local credentials, or machine-specific paths.

## Security Issues

Do not open public issues for exploitable vulnerabilities; follow
[`SECURITY.md`](SECURITY.md).

## Licensing

RaptorCode is dual-licensed under the MIT license and the Apache License 2.0
(`MIT OR Apache-2.0`). Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in RaptorCode by you shall be dual
licensed as above, without any additional terms or conditions.
