# Changelog

Date: 2026-05-19
Author: Tim
Company: RaptorLabs.dev

All notable RaptorCode changes will be documented in this file.

## v0.1.0 - Release Ready

v0.1.0 is the first open-source MVP release candidate for RaptorCode.

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
- No OpenAI, Claude, Gemini, Ollama, or LM Studio execution providers are implemented.
- No MCP, plugin system, cloud sync, desktop UI, team features, or marketplace exists.
- No enterprise policy engine or compliance certification exists.
- Persisted agent sessions and permission policy modeling are early local foundations and are frozen until after v0.1.
- DeepSeek-dependent commands require a configured `DEEPSEEK_API_KEY`.
- Implementation proposals are generated for developer review; they are not automatically applied.
