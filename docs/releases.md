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

Requires a clean working tree. The manual fallback process below documents the
same steps for forensic reference or when the script cannot run.

## Manual Fallback Process

The automated pipeline above is the normal path. The steps below are the manual
fallback, used only when `scripts/release.sh` cannot run or for forensic
reproduction of what the script does. Throughout, `X.Y.Z` is the release version
and `vX.Y.Z` its tag.

### Final verification commands

Run these before tagging (the script runs the test gate automatically):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo check --all-targets
cargo test --workspace
cargo test -p raptor-tui
CI=true cargo run --quiet -p raptor-cli --bin raptor
git diff --check
```

Build the Linux Debian installer and validate the artifacts (version-agnostic —
the `.deb` filename is derived from the current `Cargo.toml` version):

```bash
./scripts/package-deb.sh
DEB="$(ls -t dist/raptor-code_*_amd64.deb | head -1)"
appstreamcli validate --no-net --pedantic packaging/dev.raptorlabs.raptorcode.metainfo.xml
appstreamcli validate-tree --no-net "target/debian/$(basename "${DEB%.deb}")/usr"
dpkg-deb --info "$DEB"
dpkg-deb --contents "$DEB"
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

### Tagging (fallback)

Only tag after all verification passes and the maintainer explicitly approves tagging.

```bash
git status -sb
git log --oneline -5
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

### Release notes

`scripts/release.sh` publishes the GitHub Release automatically and sources its
notes from the `vX.Y.Z` section of `CHANGELOG.md`. Keep `CHANGELOG.md` current
(roll the `Unreleased` section into the new version) — it is the single source of
release notes. When publishing manually, paste the same `CHANGELOG.md` section
as the GitHub Release body.

## Release History

- `v0.1.0` (2026-05-26) — initial MVP, published under `MIT OR Apache-2.0`.
- `v0.2.0` (2026-06-16) — interactive REPL, agentic mode, live swarm, and the
  automated release pipeline. Published under
  `MIT OR Apache-2.0`.
- v0.3.0 through v0.6.5 — published under a proprietary license, with official
  binaries for Linux, macOS, and Windows distributed from https://raptorlabs.dev.
- 2026-10-31 — RaptorCode is open source again: relicensed `MIT OR Apache-2.0`
  (see `LICENSE-MIT` and `LICENSE-APACHE`), the prompt obfuscation and output
  redaction layers removed, and development moved to
  https://github.com/RaptorLabsHQ/raptorcode. The `cargo deny check` license
  gate runs in `security.yml`.
- `v0.3.0` (2026-06-25) — first cross-platform release: native binaries for
  Linux (`.deb`), macOS (universal arm64+x86_64 `tar.gz`), and Windows
  (`x86_64 .zip`), each with a SHA-256 checksum, built on native CI runners via
  `.github/workflows/release.yml` and published to the GitHub Release. Adds
  rotating prompt quotes, provider-neutral interactive status, a friendly
  context/cache usage line, and a cross-platform `run_command` shell
  (`sh -c` on Unix/macOS, `cmd /C` on Windows). macOS/Windows builds are
  unsigned (checksum-verified).
