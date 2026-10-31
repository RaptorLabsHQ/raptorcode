# Third-Party Notices

Date: 2026-06-18
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is open-source software (`MIT OR Apache-2.0`; see `LICENSE-MIT` and
`LICENSE-APACHE`). It is built on third-party open-source components that remain
governed by their own licenses, which are listed below.

RaptorCode's direct dependencies (and their transitive dependency graph) are
permissively licensed — predominantly `MIT`, `Apache-2.0`, and similarly
permissive terms (e.g. `BSD`, `ISC`, `Unicode`, `Zlib`). These licenses
permit source and binary redistribution provided their copyright and permission
notices are preserved. **No copyleft (GPL/LGPL/AGPL/MPL) license is
present in the dependency tree.**

## Direct dependencies

| Crate | Purpose | License class |
|-------|---------|---------------|
| `crossterm` | Cross-platform terminal control | MIT |
| `ratatui` | Terminal UI rendering | MIT |
| `reqwest` (rustls-tls) | HTTPS client for the DeepSeek API | MIT OR Apache-2.0 |
| `serde` | Serialization framework | MIT OR Apache-2.0 |
| `serde_json` | JSON serialization | MIT OR Apache-2.0 |

The transitive graph (≈175 crates resolved in `Cargo.lock`) carries the same
permissive license classes through `reqwest`/`rustls`, `tokio`, `hyper`, and the
`serde` ecosystem.

## Generating the complete attribution manifest

The authoritative, per-crate attribution list (with full reproduced license
texts) is generated from `Cargo.lock` at release time. Install the tooling and
regenerate before each public/GA release:

```bash
# Option A: cargo-about (recommended — emits a bundled notices document)
cargo install cargo-about
cargo about init        # one-time: creates about.toml
cargo about generate about.hbs > THIRD-PARTY-LICENSES.html

# Option B: cargo-license (quick tabular summary)
cargo install cargo-license
cargo license --avoid-build-deps --avoid-dev-deps
```

The generated `THIRD-PARTY-LICENSES.html` (or equivalent) must be shipped
alongside the binary and included in the `.deb` package under
`/usr/share/doc/raptor-code/` to satisfy the attribution requirements of the
permissive dependency licenses.

## Verifying license hygiene before release

To confirm no copyleft license enters the tree as dependencies change:

```bash
cargo install cargo-deny
cargo deny check licenses
```

This check should be wired into the release pipeline (`scripts/release.sh`)
before public GA. See `docs/security.md` and `docs/releases.md`.
