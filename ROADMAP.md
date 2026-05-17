# RaptorCode Roadmap

Date: 2026-05-17
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

## v0.1 Launch Checklist

Only these items are active before v0.1 is complete, tagged, documented, and launch-ready:

- [x] Scope audit complete
- [x] `raptor swarm` MVP
- [x] DeepSeek provider execution
- [x] `raptor chat`
- [ ] Agent role templates
- [ ] Minimal worktree file modification path
- [ ] `raptor test`
- [ ] Ratatui dashboard
- [ ] Premium terminal launch screen shown when user runs `raptor`
- [ ] DeepSeek API key onboarding prompt and fallback instruction URL
- [ ] README install/demo polish
- [ ] Release notes
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

Incomplete but required:

- Role templates for `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- Minimal model-output-to-worktree file modification path.
- `raptor test <task-id>`.
- Simple Ratatui dashboard.
- Premium startup screen for plain `raptor`.
- DeepSeek API key onboarding prompt and help URL.
- README install/demo polish.
- Release notes and `v0.1.0` tag.

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

the application must show a premium terminal launch screen before the developer prompt becomes ready. This is a v0.1 requirement, but it is not implemented in this documentation-only recovery iteration.

The launch screen must include:

- Raptor Labs company name.
- Raptor Labs URL: `https://raptorlabs.dev`.
- RaptorCode name and version display: `v0.1`.
- Premium robust RaptorCode CLI logo treatment.
- Short greeting from the Raptor Labs team wishing the developer an enjoyable, agentic, productive, creative coding session and successful delivery.
- Brand color direction: white, near-white light gray, and vivid high-saturation intense red aligned with RaptorLabs.dev.
- Exact brand hex values only after BOB adds the reference brand image to the repository or provides exact hex values.

After the launch screen, v0.1 must onboard the developer to DeepSeek:

- Prompt for the DeepSeek API key before the developer prompt is ready.
- Do not commit or log API keys.
- If the developer does not have a key, show this URL: `https://platform.deepseek.com/api_keys`.

The non-interactive `raptor chat "message"` path already requires `DEEPSEEK_API_KEY` and prints the same key URL when it is missing. The premium launch-screen interactive prompt remains incomplete.

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
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor review <task-id>
raptor diff <task-id>
raptor test <task-id>
raptor merge <task-id> --execute
```

The output must be understandable, safe, demo-ready, and aligned with the premium Raptor Labs launch experience.

## Next Implementation Prompt

PROMPT 19 - Implement agent role templates and swarm DeepSeek planning
