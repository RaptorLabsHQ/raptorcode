# Changelog

Date: 2026-05-19
Author: Bob Vasic
Company: RaptorLabs.dev

All notable RaptorCode changes will be documented in this file.

## Unreleased

### Added

- Interactive `raptor` REPL with chat and consent-gated agentic mode: DeepSeek streaming, thinking mode, reasoning effort, JSON mode, model selection, balance, usage telemetry, context compaction, and six local tools with a 120-second shell timeout.
- Interactive tool policy hooks: workspace-scoped file writes with a per-session override prompt, a hard deny-list for catastrophic shell commands, and confirm-on-prompt handling for gray-zone commands.
- Ultrathink session defaults: thinking mode enabled and maximum reasoning effort from session start in chat and agentic mode.
- Live swarm TUI dashboard with four specialist panels, live status one-liners, an activity feed, aggregated token usage, and policy confirmation modals.
- Live swarm orchestrator: `raptor swarm run "mission"` and interactive `/swarm <mission>` spawn four live specialist agents (frontend, backend, test, security) in parallel isolated worktrees with worktree-scoped tool policy; outcomes are recorded in swarm metadata and all changes are left uncommitted for review.
- Prompt IP defense layers: anti-disclosure confidentiality directives with an embedded canary token in every system prompt, a canary and verbatim-fragment leak guard that redacts system-prompt leakage from chat, agent, swarm artifact, and swarm summary output (including streamed responses), and compile-time XOR obfuscation of prompt strings at rest in the binary.

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
