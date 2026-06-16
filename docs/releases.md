# Release Process

Date: 2026-05-19
Author: Bob Vasic
Company: RaptorLabs.dev

This document defines the release process for RaptorCode.

## Automated Releases (`scripts/release.sh`)

`scripts/release.sh` is the single, authoritative way to cut a release. It
guarantees the package version can never drift from the source, because every
versioned artifact is produced by the same run.

```bash
./scripts/release.sh            # auto-derive bump from commits since last tag
./scripts/release.sh --dry-run  # print the plan, change nothing
./scripts/release.sh minor      # force a bump level
./scripts/release.sh 1.0.0      # set an explicit version
```

Version policy (default, auto-derived from Conventional Commit subjects since
the last `vX.Y.Z` tag):

- `feat!:` / `BREAKING CHANGE` → major
- `feat:` → minor
- anything else (`fix`/`chore`/`docs`/`refactor`/`test`/…) → patch

In one run the script:

1. Computes the next version (or honors an explicit level/version argument).
2. Runs the `cargo test --workspace` quality gate (skip with `--skip-tests`).
3. Synchronizes the version across `Cargo.toml`, `Cargo.lock` (via the build),
   `packaging/dev.raptorlabs.raptorcode.metainfo.xml` (new `<release>` entry),
   `CHANGELOG.md` (rolls `Unreleased` into `vX.Y.Z` with a fresh `Unreleased`),
   and `README.md` (status line + install filename).
4. Builds the Debian installer via `scripts/package-deb.sh`.
5. Commits `release: vX.Y.Z`, creates an annotated `vX.Y.Z` tag, and pushes both
   (`--no-push` to hold).
6. Publishes the GitHub Release with the `.deb` attached (`--no-release` to hold).

Requires a clean working tree. The manual steps below remain valid for
historical reference and for the one-time `v0.1.0` release.

## v0.1 Release Checklist

- [x] Confirm v0.1 scope lock.
- [x] Confirm no Phase 2 features were added for release prep.
- [x] Confirm DeepSeek is the only implemented execution provider.
- [x] Confirm README public launch wording.
- [x] Confirm changelog exists.
- [x] Confirm release notes draft exists.
- [x] Run final verification.
- [x] Review final release notes.
- [x] Tag `v0.1.0`.
- [x] Push tag to GitHub.
- [x] Create GitHub release.

## Final Verification Commands

Run these before tagging:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets
cargo test --workspace
cargo test -p raptor-tui
CI=true cargo run --quiet -p raptor-cli --bin raptor
git diff --check
```

Build the Linux Debian installer before publishing release assets:

```bash
./scripts/package-deb.sh
appstreamcli validate --no-net --pedantic packaging/dev.raptorlabs.raptorcode.metainfo.xml
appstreamcli validate-tree --no-net target/debian/raptor-code_0.1.0_amd64/usr
dpkg-deb --info dist/raptor-code_0.1.0_amd64.deb
dpkg-deb --contents dist/raptor-code_0.1.0_amd64.deb
```

Verify the installed user-facing command from the repository root:

```bash
cargo install --path crates/raptor-cli --force
raptor --help
raptor
raptor doctor
raptor tui
```

Do not run live DeepSeek commands during release verification unless `DEEPSEEK_API_KEY` is intentionally configured and the release operator confirms the network call is safe.

## Tagging Instructions

Only tag after all verification passes and BOB explicitly approves tagging.

```bash
git status -sb
git log --oneline -5
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

## GitHub Release Notes Draft

Title:

```text
RaptorCode v0.1.0
```

Body:

```markdown
RaptorCode v0.1.0 is the first open-source MVP release from Raptor Labs.

RaptorCode is a Rust-native multi-agent coding terminal for local-first supervised AI coding workflows. It uses isolated git worktrees, DeepSeek-first planning and proposal generation, developer-reviewed diffs, supervised tests, and explicit merge control.

Highlights:
- `raptor` startup screen with Raptor Labs / Raptor Code branding.
- `raptor tui` read-only Ratatui dashboard.
- `raptor chat` DeepSeek chat smoke test.
- `raptor swarm` role worktrees for frontend, backend, test, and security agents.
- `RAPTOR_PLAN.md` planning artifacts per role.
- `raptor swarm implement` supervised implementation proposal artifacts.
- Role-aware `raptor review` and `raptor diff`.
- `raptor test` supervised test execution inside validated worktrees.
- Merge dry-run and explicit local merge execution.

Safety:
- DeepSeek is the only implemented execution provider in v0.1.
- API keys are read from `DEEPSEEK_API_KEY` and are not printed or stored.
- Model output is not executed.
- Patches are not applied automatically.
- The dashboard is read-only.
- Policy enforcement hooks are not implemented in v0.1.

Known limitations:
- v0.1 is not a full terminal emulator replacement.
- No desktop app, cloud sync, team features, marketplace, MCP, or enterprise policy engine exists.
- OpenAI, OpenRouter, Gemini, Ollama, and LM Studio execution providers are not implemented.
```
