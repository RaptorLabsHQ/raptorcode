# Changelog

Date: 2026-05-19
Author: Bob Vasic
Company: RaptorLabs.dev

All notable RaptorCode changes will be documented in this file.

## Unreleased

### Added

- **Continuous integration workflow (`.github/workflows/ci.yml`).** Every push
  and pull request now runs `cargo fmt --check`, `cargo clippy -D warnings`,
  and the full `cargo test --workspace` suite on native Linux, macOS, and
  Windows runners. Previously the only automated gate was the release script's
  test run, and Windows/macOS code paths were exercised only at release time.
  Dependabot now files weekly grouped updates for cargo and GitHub Actions
  dependencies.

### Fixed

- **`raptor task cleanup` now prunes the git worktree registration.** Cleanup
  previously deleted only the worktree directory, leaving a stale prunable
  entry under `.git/worktrees` that git kept listing and that blocked re-adding
  a worktree at the same path. `execute_task_worktree_cleanup` now runs
  `git worktree prune` in the repository root after deleting the directory.
  Branch retention, metadata retention, and remote behavior are unchanged.

## v0.4.0 - 2026-06-28

### Changed

- **Split `lib.rs` into focused modules: `repl`, `render`, and `task_commands`.**
  The crate root previously mixed CLI parsing/dispatch, the interactive terminal,
  the task/swarm command handlers, and all presentation. Those concerns now live
  in dedicated sibling modules — `repl` (the interactive read-eval-print loop,
  agent loop, spinner, prompt input, credential storage), `render` (all `render_*`
  result formatters and shared number/path/diff helpers), and `task_commands`
  (the `raptor task`/`raptor swarm`/review/diff/merge handlers). `lib.rs` is now a
  thin root (~940 lines of production code) holding the CLI types, `parse_args`,
  the `run` dispatch, and module wiring. Handlers are re-exported (`pub use`) so
  dispatch and tests call them unqualified; no behavior change.
- **Extracted the task & swarm command layer into `task_commands.rs`.** The
  `raptor task`/`raptor swarm`/review/diff/test/merge command handlers and their
  `render_*` presentation helpers (~1.5k lines) moved out of `lib.rs` into a
  dedicated module, shrinking `lib.rs` from ~7.0k to ~5.5k lines (-22%). Generic
  display utilities stay in `lib.rs`, shared via `pub(crate)`. The handlers are
  re-exported (`pub use task_commands::*`) so dispatch and tests keep calling
  them unqualified. No behavior change.
- **De-duplicated the DeepSeek SSE streaming readers.** The chat and agent
  streaming response readers each carried their own copy of the Server-Sent
  Events framing loop (line iteration, blank/`:`-comment skipping, `data:`
  stripping, `[DONE]` termination). That framing now lives in one
  `for_each_sse_data_line` helper that both readers share, and the chat reader's
  per-chunk assembly moved into a tested `apply_deepseek_chat_stream_data`
  mirroring the agent path. New unit test covers chat-stream content/reasoning/
  model/usage accumulation, which previously had no direct coverage. No behavior
  change.
- **Extracted interactive slash-command handling into `slash_command.rs`.** The
  REPL's `/swarm`, `/exit`, `/help`, `/clear`, `/thinking`, `/effort`, `/stream`,
  `/json`, `/balance`, `/api`, and `/model` commands moved out of the ~7.2k-line
  `lib.rs` REPL loop into a dedicated module: a pure `parse()` classifier (now
  unit-tested for case sensitivity, the `/swarm` word boundary, and aliases) plus
  a `dispatch()` that runs each handler against a borrowed `SlashSession`. The
  main loop now just constructs the session and acts on the returned
  `ReplControl`. Behavior is unchanged.

### Added

- **Live swarm dashboard footer quote now rotates during the run.** Previously a
  single quote was chosen once and frozen for the whole run; the dashboard now
  cycles through the embedded quote pool every 8 seconds. The orchestrator passes
  the full pool (rotated to a pseudo-random start), and `raptor-tui` advances it
  on a timer inside the render loop, keeping the crate free of the quote asset.

### Changed

- **Windows hardening for the credentials file and command timeout.** The saved
  DeepSeek API key now gets a `0600`-equivalent ACL on Windows: after writing
  `~/.raptor/.credentials`, `icacls /inheritance:r /grant:r <user>:F` strips
  inherited ACEs and grants full control to the current user only (hard-fails
  like the Unix `chmod`, so a key is never left readable by other principals).
  The credentials directory now also resolves on Windows via `%USERPROFILE%`
  (falling back from `$HOME`), which previously left the path unresolved. The
  `run_command` 120-second timeout now terminates the whole process tree on
  Windows with `taskkill /T /F /PID` (mirroring the Unix process-group kill)
  instead of `child.kill()`, which only killed `cmd.exe` and orphaned any
  child dev servers it spawned.

## v0.3.0 - 2026-06-25

### Changed

- **Relicensed to proprietary, closed-source.** RaptorCode is no longer open
  source. `LICENSE` is now the RaptorLabs Proprietary EULA (reverse engineering,
  decompilation, redistribution, and derivative works prohibited; trade-secret
  and confidentiality protection asserted over source and system prompts).
  `Cargo.toml` and the AppStream metainfo carry a proprietary license id, and
  `publish = false` is set workspace-wide. Releases up to and including v0.2.0
  remain validly `MIT OR Apache-2.0` for their recipients; the proprietary terms
  apply to all releases after v0.2.0.
- Reframed README and all docs from open-source to proprietary, closed-source,
  binary-distribution framing. Official distribution is signed binaries for
  Linux, macOS, and Windows from https://raptorlabs.dev; source builds are
  restricted to authorized RaptorLabs developers.

### Added

- `THIRD-PARTY-NOTICES.md` documenting attribution obligations for the bundled
  permissive open-source dependencies and how to regenerate the full manifest.
- Release-profile hardening in `Cargo.toml` (`strip = "symbols"`, `lto = true`,
  `codegen-units = 1`) to raise reverse-engineering cost; panic strategy left as
  `unwind` to preserve live-swarm thread isolation.
- `docs/security.md` Closed-Source Intellectual Property Protection section
  documenting the legal and technical IP controls and their honest limits.
- Rotating saved quotes on a reserved line beneath the input box while the
  developer types and submits a prompt, for interactivity. Rendered as a single
  atomic frame write so it never displaces typed input.
- Cross-platform support for the agent `run_command` tool: the host shell is now
  selected per platform (`sh -c` on Unix/macOS, `cmd /C` on Windows), making the
  tool functional on Windows. Closes the last Windows runtime port gap.

### Changed

- Interactive status no longer names the model provider: the streaming line reads
  "Streaming agent response..." and per-step token telemetry is shown as a
  friendly "Context: N tokens · X% cached" line instead of raw
  prompt/completion/cache-hit/cache-miss counts. Prefix-cache "misses" are simply
  the new tokens added each turn, not failures.

## v0.2.0 - 2026-06-16

### Added

- Interactive `raptor` REPL with chat and consent-gated agentic mode: DeepSeek streaming, thinking mode, reasoning effort, JSON mode, model selection, balance, usage telemetry, context compaction, and six local tools with a 120-second shell timeout.
- Interactive tool policy hooks: workspace-scoped file writes with a per-session override prompt, a hard deny-list for catastrophic shell commands, and confirm-on-prompt handling for gray-zone commands.
- Ultrathink session defaults: thinking mode enabled and maximum reasoning effort from session start in chat and agentic mode.
- Live swarm TUI dashboard with four specialist panels, live status one-liners, an activity feed, aggregated token usage, and policy confirmation modals.
- Live swarm orchestrator: `raptor swarm run "mission"` and interactive `/swarm <mission>` spawn four live specialist agents (frontend, backend, test, security) in parallel isolated worktrees with worktree-scoped tool policy; outcomes are recorded in swarm metadata and all changes are left uncommitted for review.
- Prompt IP defense layers: anti-disclosure confidentiality directives with an embedded canary token in every system prompt, a canary and verbatim-fragment leak guard that redacts system-prompt leakage from chat, agent, swarm artifact, and swarm summary output (including streamed responses), and compile-time XOR obfuscation of prompt strings at rest in the binary.
- Automated versioned release pipeline (`scripts/release.sh`): derives the next semantic version from Conventional Commit history since the last tag, synchronizes that version across `Cargo.toml`, the AppStream metainfo, `CHANGELOG.md`, and `README.md`, builds the Debian installer, then commits, tags `vX.Y.Z`, pushes, and publishes the GitHub Release with the `.deb` attached. This makes the package version impossible to drift from the source.

### Safety Notes

- Live swarm agents and interactive agentic mode execute model-requested tools only after explicit developer consent and under the interactive tool policy; the planning and proposal swarm paths remain non-executing.
- Prompt obfuscation and output redaction raise extraction cost but are not cryptographic protection; see docs/security.md Prompt IP Defense Layers for limitations.

## v0.1.0 - Released

v0.1.0 is the first open-source MVP release for RaptorCode.

### Added

- Rust workspace with the `raptor` CLI binary.
- Premium Raptor Labs / Raptor Code startup screen.
- Simple read-only Ratatui dashboard through `raptor tui`.
- Safe `raptor init` project configuration creation.
- `raptor doctor` local diagnostics.
- DeepSeek-first `raptor chat` command using `DEEPSEEK_API_KEY`.
- `raptor provider list` metadata inspection.
- Local agent metadata commands: `agent list`, `agent show`, `agent create`, and `agent policy`.
- Single-task worktree foundation through `raptor task`.
- Task metadata inspection, abandonment, and safe cleanup.
- Multi-role swarm worktree creation through `raptor swarm`.
- DeepSeek planning artifacts written as `RAPTOR_PLAN.md` per swarm role.
- Supervised implementation proposal generation through `raptor swarm implement`.
- Proposal artifacts written as `RAPTOR_IMPLEMENTATION_PROPOSAL.md`.
- Role-aware `raptor review` and `raptor diff`.
- Supervised `raptor test` execution inside validated task or role worktrees.
- Merge dry-run and explicit supervised local merge execution.

### Safety Notes

- DeepSeek is the only implemented execution provider in v0.1.
- API keys are read only from `DEEPSEEK_API_KEY`.
- API keys are not printed, stored, committed, or read from `.env` files.
- Model output is treated as untrusted text.
- Model output is not executed.
- Patches are not applied automatically.
- Source files are not automatically modified by model output.
- Work happens inside validated local git worktrees.
- The TUI dashboard is read-only and performs no model calls or file writes.
- Policy enforcement hooks are not implemented in v0.1.

### Known Limitations

- v0.1 is a local-first supervised MVP, not a full terminal emulator replacement.
- No OpenAI, OpenRouter, Gemini, Ollama, or LM Studio execution providers are implemented.
- No MCP, plugin system, cloud sync, desktop UI, team features, or marketplace exists.
- No enterprise policy engine or compliance certification exists.
- Persisted agent sessions and permission policy modeling are early local foundations and are frozen until after v0.1.
- DeepSeek-dependent commands require a configured `DEEPSEEK_API_KEY`.
- Implementation proposals are generated for developer review; they are not automatically applied.
