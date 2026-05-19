# RaptorCode Roadmap

Date: 2026-05-18
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

## v0.1 Launch Checklist

Only these items are active before v0.1 is complete, tagged, documented, and launch-ready:

- [x] Scope audit complete
- [x] `raptor swarm` MVP
- [x] DeepSeek provider execution
- [x] `raptor chat`
- [x] Agent role templates
- [x] Swarm DeepSeek planning artifacts
- [x] Minimal worktree file modification path
- [x] Role-aware swarm review and diff
- [x] `raptor test`
- [x] Ratatui dashboard
- [x] Premium terminal launch screen shown when user runs `raptor`
- [x] DeepSeek API key environment detection and fallback instruction URL
- [x] README install/demo polish
- [x] Changelog and release process draft
- [ ] Final verification
- [ ] Release notes final review
- [ ] Tag `v0.1.0`

## v0.1 Scope Lock

v0.1 scope is strictly:

```text
CLI + TUI + Agent Orchestrator
```

The only active implementation path is:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

No Phase 2 feature work is allowed before v0.1. This explicitly excludes advanced persisted agent sessions, advanced permission policy engines, policy enforcement hooks, additional providers, cloud features, plugin systems, MCP, desktop terminal work, marketplace features, and team features.

The persisted agent sessions and permission policy added in commit `483135d7e8e8d25de726874eaddfaa7338f77fe0` are implemented early but frozen until after v0.1. They should be kept if tests pass and should not be expanded unless BOB explicitly reopens that scope after v0.1.

The v0.1 binary remains `raptor`. A `raptorcode` alias can be considered later only after BOB explicitly approves that naming change.

## Current v0.1 Status

Complete and in-scope:

- Rust workspace and `raptor` CLI binary.
- CLI command router.
- `raptor init`.
- `raptor doctor`.
- Single-task worktree foundation through `raptor task "description"`.
- Task metadata list/show.
- Read-only worktree review and diff commands.
- Supervised merge preflight and local merge execution.
- Basic local safety checks for task ids, worktree paths, and merge confirmation.
- `raptor swarm` role-based worktree MVP with `.raptor/swarms/<task-id>.toml` metadata.
- DeepSeek provider execution through `DEEPSEEK_API_KEY`.
- `raptor chat "message"` smoke-test command for DeepSeek completion.
- Role templates for `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- `raptor swarm "description"` DeepSeek planning that writes `RAPTOR_PLAN.md` per role when `DEEPSEEK_API_KEY` is set.
- Missing `DEEPSEEK_API_KEY` blocks planning status without failing swarm worktree creation.
- `raptor swarm implement <task-id> <role>` supervised implementation proposal generation.
- `RAPTOR_IMPLEMENTATION_PROPOSAL.md` artifacts inside selected role worktrees.
- `raptor review <task-id> [role]` and `raptor diff <task-id> [role]` inspect swarm role worktrees read-only.
- `raptor test <task-id>` runs detected or explicit test commands inside validated task or swarm role worktrees.
- Base `raptor` renders the premium Raptor Labs / Raptor Code startup screen.
- `raptor tui` opens the simple read-only Ratatui dashboard.
- Startup detects `DEEPSEEK_API_KEY` and prints the DeepSeek key URL when missing without storing or printing keys.

Remaining before tag:

- Final verification.
- Release notes final review.
- Tag `v0.1.0`.

Implemented early but frozen:

- Persisted `.raptor/agents/<agent-id>.toml` sessions.
- `raptor agent create`.
- Persisted `raptor agent list` and `raptor agent show <id>`.
- `raptor agent policy`.
- Deny-by-default permission policy modeling.

## Mandatory v0.1 Launch UX

When a developer runs:

```bash
raptor
```

the application shows a premium terminal launch screen before the developer prompt becomes ready. This v0.1 requirement is implemented for the existing `raptor` binary.

The launch screen must include:

- Raptor Labs company name.
- Raptor Labs URL: `https://raptorlabs.dev`.
- Raptor Code name and version display: `v0.1`.
- Premium compact Raptor Code CLI logo treatment.
- Short greeting from the Raptor Labs team wishing the developer an enjoyable, agentic, productive, creative coding session and successful delivery.
- Brand colors from the v0.1 UI specification: `#0A0A0A`, `#F4F4F4`, `#AAAAA9`, `#636262`, `#FF1919`, `#A51D20`, and `#261313`.

After the launch screen, v0.1 onboards the developer to DeepSeek by detecting `DEEPSEEK_API_KEY`. If the key exists, startup reports that it is detected without printing it. If the developer does not have a key, startup shows `https://platform.deepseek.com/api_keys`. RaptorCode does not store API keys, prompt for persistence, read `.env` files, or log secrets.

## Non-Goals Before v0.1

- Full terminal emulator replacement.
- Advanced agent memory.
- Enterprise policy engine.
- Policy enforcement hooks.
- Additional model providers beyond DeepSeek execution.
- OpenAI provider implementation.
- Claude provider implementation.
- Gemini provider implementation.
- Ollama provider implementation.
- Desktop terminal.
- Cloud features.
- Plugin systems.
- MCP.
- Marketplace features.
- Team features.

## Release Criteria

v0.1 is complete when this supervised local workflow works and is documented:

```bash
raptor init
raptor
raptor tui
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor swarm implement <task-id> frontend-agent
raptor review <task-id>
raptor review <task-id> frontend-agent
raptor diff <task-id>
raptor diff <task-id> frontend-agent
raptor test <task-id>
raptor merge <task-id> --execute
```

The output must be understandable, safe, demo-ready, and aligned with the premium Raptor Labs launch experience.

## Next Implementation Prompt

PROMPT 25 - Final verification and tag v0.1.0
