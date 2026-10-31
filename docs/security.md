# Security Design

Date: 2026-05-18
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source, Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Security Model

RaptorCode assumes AI coding agents can make incorrect or unsafe changes. The system must keep developers in control and make agent actions inspectable before they affect the main working tree or remote repository.

For v0.1, security work must stay focused on the launch path:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

Do not expand policy enforcement hooks, enterprise policy systems, advanced agent memory, additional providers, cloud features, plugin systems, or MCP before v0.1.

## Primary Controls

- Worktree isolation for concurrent agent changes.
- Explicit confirmation for destructive operations.
- Clear status reporting before commit, merge, or push operations.
- Path validation for filesystem operations.
- Structured command execution instead of shell string concatenation.
- Minimal dependencies with documented justification.
- Deterministic task identifiers derived from validated task descriptions.
- Local `.raptor/` state ignored by git to avoid accidentally committing runtime metadata or nested worktrees.

## Auditability

Security-relevant actions should be logged or surfaced clearly to the developer:

- Worktree creation and deletion.
- Agent task assignment.
- Command execution requests.
- Commit, merge, and push attempts.
- Failed validation checks.

## Initial Threats

- Path traversal through untrusted task input.
- Shell injection through generated commands.
- Accidental overwrite of user work.
- Unsafely merging unreviewed agent output.
- Uncontrolled model or provider execution.
- Secret leakage through provider configuration.
- Leaking secrets through logs or generated files.
- Model output proposing unsafe commands or hidden file changes.

## Provider And Agent Foundation Controls

`raptor provider list`, `raptor agent list`, `raptor agent show <id>`, and `raptor agent policy` are inspection-only commands. `raptor agent create` writes agent metadata only.

Current provider controls:

- Base `raptor` checks whether `DEEPSEEK_API_KEY` or a saved local DeepSeek key is present, never prints the raw key, and never reads `.env` files.
- Interactive key onboarding stores the user-provided DeepSeek key in `~/.raptor/.credentials` with owner-only permissions: `0600` on Unix/macOS, and on Windows a `0600`-equivalent ACL applied via `icacls /inheritance:r /grant:r <user>:F` (inherited ACEs stripped, full control granted to the current user only). Repository files and `raptor.toml` remain secrets-free.
- Base `raptor` prints `https://platform.deepseek.com/api_keys` when the DeepSeek key is missing.
- Base `raptor` uses minimal plain-text output for non-TTY output and `CI=true`; `NO_COLOR` disables ANSI color.
- `raptor tui` is read-only and does not call models, run shell commands, modify files, write metadata, commit, merge, or push.
- Provider ids are validated before configuration is accepted.
- Provider metadata is built in and local.
- DeepSeek is the only provider that may receive real execution before v0.1.
- `raptor.toml` supports provider enablement metadata only.
- No API keys, tokens, secrets, endpoints, or credentials are stored in `raptor.toml`.
- DeepSeek API keys must never be committed, stored in repository files, or logged.
- `raptor chat "message"` reads `DEEPSEEK_API_KEY` from the environment only.
- If `DEEPSEEK_API_KEY` is missing, `raptor chat` prints `https://platform.deepseek.com/api_keys`.
- `raptor chat` sends only a fixed system prompt and the user-provided message to DeepSeek.
- `raptor chat` does not send repository contents, `.env` files, task metadata, worktree files, or secrets.
- `raptor chat` does not execute model output, run shell commands, write source files, create worktrees, merge, commit, or push.
- Interactive base `raptor` agentic mode is different from `raptor chat`: after explicit file-system consent it may execute model-requested local tools, including file writes and shell commands, inside the current agent working directory.
- `raptor swarm` sends only the task description and v0.1 role template to DeepSeek when `DEEPSEEK_API_KEY` is set.
- `raptor swarm` writes only `RAPTOR_PLAN.md` planning artifacts inside assigned role worktrees.
- `raptor swarm` records `blocked_missing_deepseek_api_key` without destroying worktrees when `DEEPSEEK_API_KEY` is missing.
- Swarm plans are AI-generated and must be reviewed before implementation.
- `raptor swarm` does not execute model output, run shell commands, apply patches, modify application source files, run tests, merge, commit, or push.
- `raptor swarm implement <task-id> <role>` sends only the task description, selected role, and selected role `RAPTOR_PLAN.md` to DeepSeek.
- `raptor swarm implement <task-id> <role>` writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside the selected role worktree.
- Implementation proposals are AI-generated and must be reviewed before source changes are applied manually.
- `raptor swarm implement <task-id> <role>` does not execute model output, run shell commands, apply patches, modify application source files, run tests, merge, commit, or push.
- `raptor test <task-id>` calls no AI provider and sends no data to models.
- `raptor test <task-id>` executes only detected or developer-supplied test commands inside validated task or swarm role worktrees.
- Explicit test commands are structured process arguments, not shell strings.
- Explicit test commands are limited to test-oriented subcommands and reject path traversal arguments.
- `raptor test <task-id>` does not install dependencies, execute model output, update metadata, edit source files, commit, merge, or push.
- Streaming is implemented for interactive chat and agent responses. Streamed DeepSeek agent tool-call deltas are assembled before local tool execution. No provider SDKs or additional provider executions are implemented before v0.1.

## Interactive Agent Tool Controls

Date: 2026-06-05
Author: Bob Vasic
Company: RaptorLabs.dev

Interactive agentic mode has a broader trust boundary than the metadata-only `raptor agent` commands.

- Agentic mode starts only after an explicit local file-system consent prompt.
- Available tools are `write_file`, `read_file`, `list_directory`, `create_directory`, `change_directory`, and `run_command`.
- `run_command` uses a shell command string from the model, so the consent prompt must be treated as granting privileged local execution for that session.
- Shell commands run with a 120-second timeout and process-tree termination to reduce orphaned process risk: Unix process-group `kill` on Unix/macOS, and `taskkill /T /F /PID` on Windows (the std `child.kill()` only terminates `cmd.exe`, leaving any spawned child processes orphaned).
- Tool results are appended to provider history as tool messages for auditability inside the active session.
- DeepSeek usage telemetry is monitored; context warnings and local compaction reduce runaway context growth.

## Interactive Tool Policy Hooks

Date: 2026-06-12
Author: Bob Vasic
Company: RaptorLabs.dev

Tool execution now lives in the dedicated `raptor-cli::agent_tools` module, and every interactive agent tool call passes through `raptor-cli::tool_policy` before execution. The policy decisions are enforced in the shared `raptor-cli::agent_loop` before any execution, for the interactive REPL and every live-swarm worker alike, so confirmation prompts are interactive and auditable.

File write policy:

- `write_file` and `create_directory` targets are resolved against the agent working directory and lexically normalized (`.` and `..`) so non-existent targets can be compared.
- Writes inside the workspace root (the directory where `raptor` was launched, canonicalized at session start) are allowed.
- The first write attempt outside the workspace prompts the developer explicitly; approval applies for the rest of the session, denial returns a policy error to the model without executing.
- `read_file` and `list_directory` remain unrestricted reads under the existing session consent.
- Known limitation: normalization is lexical, so a symlink inside the workspace pointing outside it can bypass the boundary; shell commands can also write anywhere, which the command policy below only partially mitigates.

Shell command policy (`run_command`):

- Hard deny-list, never executed: recursive deletion of the filesystem root (including `--no-preserve-root` and `/*`), `mkfs*`, `dd of=/dev/*`, output redirection onto block devices, recursive `chmod`/`chown` on `/`, and fork-bomb patterns. Denials are reported to the developer and returned to the model as policy errors.
- Confirm-on-prompt gray zone: `sudo`/`doas`, recursive force deletes targeting absolute paths or the home directory, `git push --force` (including `--force-with-lease`), and power-cycle commands (`shutdown`, `reboot`, `poweroff`, `halt`). The developer approves or denies each occurrence; denial returns a policy error to the model.
- Commands are split on shell separators and each segment is evaluated; the most severe decision wins, and `sudo`-wrapped segments are also evaluated without the prefix so hard denials cannot hide behind privilege escalation.
- Everything else executes as before under the session consent, 120-second timeout, and process-group termination.

The deny-list is intentionally small and unambiguous. It is a safety net for catastrophic operations, not a sandbox; the per-session consent prompt remains the primary trust boundary, and `raptor-sandbox` remains the future home for stronger isolation.

Current agent controls:

- Agent ids are validated before lookup or persistence.
- Agent ids, task ids, provider ids, and model ids are treated as untrusted input.
- Agent sessions are local-only and read-only in this foundation.
- `raptor agent create` writes only `.raptor/agents/<agent-id>.toml` metadata and refuses duplicate deterministic ids.
- `raptor agent list` reads persisted sessions only from `.raptor/agents/`.
- `raptor agent show <id>` reads only `.raptor/agents/<agent-id>.toml`, validates path-safe ids, and returns a clear not-found error when no session exists.
- Read-only agent execution mode does not allow file modification, shell execution, or model calls.
- Agent session models support timestamps and future audit entries, but no autonomous coding loop exists.
- Agent creation does not call models, execute shell commands, write source files, run provider APIs, install dependencies, or push remotes.

Current permission policy controls:

- Default approval mode is manual.
- File-read permission is modeled as repository-root-only for future use.
- File write is denied.
- Shell execution is denied.
- Network access is denied.
- Model execution is denied until explicitly configured and invoked in a future design.
- Dangerous commands are denied.
- The policy is display/modeling only in this iteration; it does not grant tool execution.
- This permission policy code was implemented early and is frozen until after v0.1 unless a narrow change is directly required for the v0.1 demo.
- Policy enforcement hooks must not be implemented before v0.1.

## Tool Policy Hardening

Date: 2026-08-16
Author: Bob Vasic
Company: RaptorLabs.dev

The interactive and live-swarm tool policy (`raptor-cli::tool_policy`) is now deny-by-default and gates network egress. RaptorCode remains a local-trust tool: the per-session consent prompt is the primary boundary, and these controls are backstops, not a sandbox.

- **Deny-by-default dispatch.** `ToolPolicy::evaluate` permits only the known tool set. `write_file` and `create_directory` go through the write-boundary check, `run_command` through the shell-command policy, and `read_file`, `list_directory`, and `change_directory` are permitted as read-only/navigation tools. Any other tool name is denied and reported to the model as a policy error, so a tool added to `build_agent_tools` without a matching policy arm is refused until its policy is written instead of executing unpoliced.
- **Network egress confirmation.** Shell segments whose command is a network tool (`curl`, `wget`, `nc`/`ncat`/`netcat`, `telnet`, `ssh`, `scp`, `sftp`, `rsync`, `ftp`) require explicit developer confirmation before execution. Reads stay unrestricted under session consent, so this confirmation is the control that keeps a read secret from being silently sent off the machine. It is first-token matching only — an interpreter one-liner (`python -c ...`) or a shell `/dev/tcp` redirect can still reach the network — so it raises cost rather than closing the channel. Network confinement (a network namespace) remains future `raptor-sandbox` work.

Security assumption: because there is no hosting and the agent runs on the developer's own machine under explicit consent, OS-level isolation is intentionally out of scope for this iteration. These backstops reduce accidental and low-effort-malicious blast radius; they are not a defense against a determined actor with shell access the developer has already authorized.

### Review Hardening (2026-10-02)

Date: 2026-10-02
Author: Bob Vasic
Company: RaptorLabs.dev

Findings from the full architecture review (`docs/architecture-review-2026-10-02.md`) applied to the tool boundary:

- **Shell wrappers are unwrapped.** The deny-list judged a segment by its first token, so `bash -c "rm -rf /"`, `sh -lc '...'`, `nohup rm -rf /`, `timeout 30 rm -rf /`, `env X=1 rm -rf /`, `eval`, and `exec` were allowed. `evaluate_shell_segment` now recognizes `sh`/`bash`/`zsh`/`dash`/`ksh -c`, `env`, `timeout`, `nohup`, `nice`, `time`, `eval`, `exec`, and `command`, and evaluates the wrapped command as a whole (so `;`/`&&` inside the quoted string are split and each part judged). Quote stripping trims unmatched leading/trailing quotes so the words of a split quoted string are analyzed bare. Still lexical: `$(...)`, backticks, `xargs`, `find -delete`, and interpreter one-liners remain out of scope, as documented above.
- **`run_command` drains output and closes stdin.** stdout and stderr are read on background threads from spawn, so a command producing more than the kernel pipe buffer can no longer deadlock into a false timeout; stdin is `/dev/null`, so a command that prompts fails fast instead of hanging, and live-swarm workers never read the dashboard's terminal input.
- **`read_file` is capped** at 200,000 bytes with an explicit truncation marker, bounding how much one call can place in model context.
- **Credentials file is owner-only from creation.** `~/.raptor/.credentials` is opened with mode `0600` (Unix) rather than written then chmodded, removing the umask-dependent window where the key was world-readable; an existing looser file is tightened on rewrite. Windows keeps the `icacls` ACL step.
- **One policy-gated loop.** The interactive REPL and the live-swarm workers previously ran two copies of the tool-calling cycle, so a policy fix could land in one and not the other. Both now run `raptor-cli::agent_loop`, which evaluates `ToolPolicy` before every execution; the callers only differ in how a confirmation is asked (stdin prompt vs. dashboard modal) and how progress is shown. Nine loop tests with a scripted provider cover deny, confirm-declined, confirm-approved (and the per-session outside-write unlock), step limit, cancellation, and provider errors.

A verification pass over the delivered slices (correctness, security, TUI, and CI lenses with adversarial checks) added, the same day:

- **Wrapper options, program paths, and prefixes are seen through.** Each wrapper skips its own options and the values they take before the inner command is read (`timeout -s TERM 30 cargo test`, `nice -n 10 cargo build`, `env -u HOME RUST_LOG=debug cargo run`, `sh -c -- cargo test`, `flock -n build.lock cargo build`), the program is matched by basename (`/bin/sh -c …`, `/usr/bin/nice …`), leading `NAME=value` assignments are dropped, `sudo`/`doas` skip their options, and `su -c` is privileged like them. Newly recognized wrappers: `setsid`, `stdbuf`, `ionice`, `chrt`, `taskset`, `unshare`, `flock`, `busybox`. A shell with no `-c` and no script file (`sh`, `bash -i`, `sh -s`) reads its commands from stdin, which the policy cannot inspect, so it asks for confirmation. Wrapper nesting is bounded at 16 levels; deeper chains are denied rather than followed. Redirect targets and `dd of=` values are unquoted before the device check. Comment and shebang lines (heredoc data), `bash --version`, and `command -v` stay allowed. Known cost: the splitter is not quote-aware, so a search pattern whose last alternative is a gated word (`rg "reqwest|curl" src`) asks for confirmation; a quote-aware splitter is the proper fix.
- **`read_file` reads regular files only.** Directories, FIFOs, and device files (`/dev/zero` never reaches EOF) are refused up front; at most the cap plus one byte is read, binary content is refused instead of being handed to the model as mojibake, and a cut through a multi-byte character at the cap is handled without a replacement character.
- **`run_command` returns when the shell exits** even if a backgrounded child (a dev server) keeps the output pipes open: after a 2-second grace the output captured so far is returned with a note that a background process is still running.
- **`/api` → "Show current API Key" prints no key characters.** It reports only whether a key is configured and where keys are saved; the former masked form still put twelve key characters into scrollback and screen shares. This also resolves CodeQL's `rust/cleartext-logging` alert on that display.
- **The credentials file is tightened before the key is written.** The Unix mode (or the Windows ACL) is applied on the open handle before `write_all`, so rewriting an existing looser file never exposes the new key.

## Live Swarm Agentic Mode

Date: 2026-06-12
Author: Bob Vasic
Company: RaptorLabs.dev

`raptor swarm run "mission"` and the interactive `/swarm <mission>` REPL command launch the live multi-agent implementation path. Unlike the planning-only and proposal-only swarm paths, live swarm agents execute model-requested tools.

- `/swarm` is available only after the per-session agentic file-system consent; `raptor swarm run` refuses non-interactive terminals and `CI=true` environments.
- The live swarm refuses to run when task or swarm metadata already exists for the derived task id.
- Each role agent runs on its own thread inside its own isolated git worktree under `.raptor/worktrees/<task-id>/<role>/`, created through the validated worktree creation path.
- Each worker constructs its own `ToolPolicy` rooted at its canonicalized worktree: writes outside the worktree require explicit developer confirmation, the catastrophic-command hard deny-list applies unchanged, and gray-zone shell commands require per-occurrence confirmation.
- Policy confirmations are forwarded to the dashboard as modal prompts with a reply channel; denial returns a policy error to that agent's model. Quitting the dashboard (`q`/`Esc`) denies queued confirmations and requests a cooperative stop honored at step and tool-call boundaries.
- Each agent is limited to 100 steps; `run_command` keeps the 120-second timeout with Unix process-group termination.
- Prompt context per agent is limited to the fixed role system prompt, the developer mission text, and that agent's own conversation and tool-result history; API keys and repository secrets are never included.
- Live agents reuse the context budget guard, so per-agent history warns and compacts before DeepSeek's documented context limit.
- Worker outcomes (completed, aborted, failed, files written, steps) are recorded in `.raptor/swarms/<task-id>.toml` with live-run lifecycle statuses for auditability.
- All resulting changes remain uncommitted inside role worktrees for developer review via `raptor review`, `raptor diff`, `raptor test`, and the supervised merge path.

## Prompts And Model Output

Date: 2026-10-31
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is open source, so its system prompts are plain string constants in
`raptor-cli::prompts` and ship in the binary as-is. Every composed prompt is
the mode base text plus caller context (the current working directory, the
mission text, the task description); API keys and repository secrets are never
part of a prompt. Model output is rendered unmodified: there is no
anti-disclosure directive, canary token, output redaction, or at-rest prompt
obfuscation. Prompt injection remains an industry-wide, unsolved problem; the
controls that matter here are the ones around tool execution above (consent,
the tool policy, worktree isolation, and the supervised merge path), not
secrecy of the prompt text.

Release-profile hardening in `Cargo.toml` (`strip = "symbols"`, `lto = true`,
`codegen-units = 1`) is kept for binary size and performance. The panic
strategy is intentionally left as `unwind` (not `abort`) so a single live-swarm
worker panic stays isolated to its thread.

## v0.1 Model And Worktree Safety

DeepSeek output remains untrusted.

Before v0.1 launch:

- Never execute commands automatically from model output.
- Never install dependencies automatically from model output.
- Never write files outside assigned worktrees.
- Never write swarm planning artifacts outside `.raptor/worktrees/<task-id>/<role>/RAPTOR_PLAN.md`.
- Never write swarm implementation proposal artifacts outside `.raptor/worktrees/<task-id>/<role>/RAPTOR_IMPLEMENTATION_PROPOSAL.md`.
- Never perform hidden file writes.
- Never include `.env`, API keys, credentials, or secrets in model context.
- Never commit or log `DEEPSEEK_API_KEY`.
- Never store `DEEPSEEK_API_KEY` in `raptor.toml` or `.raptor/`.
- Show diffs before merge.
- Keep merge supervised.
- Keep generated code reviewable by the developer.

## Implemented Task Workflow Controls

`raptor task "description"` currently applies these controls:

- Requires a non-empty task description.
- Generates a deterministic safe task id from the description.
- Validates branch and worktree names before git operations.
- Inspects the git repository root before creating task state.
- Creates worktrees only under `.raptor/worktrees/`.
- Refuses path traversal and unsafe names.
- Refuses to overwrite existing worktree paths.
- Refuses to overwrite existing task metadata.
- Persists task metadata under `.raptor/tasks/`.
- Reads task metadata only from validated `.raptor/tasks/<id>.toml` paths.
- Rejects unsafe task ids and path traversal attempts in task inspection commands.
- Validates stored worktree paths against `.raptor/worktrees/<task-id>` before review or diff inspection.
- Uses read-only git status and diff commands for `raptor review <task-id>` and `raptor diff <task-id>`.
- Falls back to swarm metadata for `raptor review <task-id> [role]` and `raptor diff <task-id> [role]` when no single-task metadata exists.
- Validates swarm role worktree paths against `.raptor/worktrees/<task-id>/<role>/` before role-aware review or diff inspection.
- Runs `raptor merge <task-id>` as a preflight dry run only.
- Refuses merge preflight when the target repository is dirty, the task worktree has no changes, or unresolved conflicts are present.
- Runs `raptor merge <task-id> --execute` only after exact typed confirmation.
- Refuses merge execution when branches cannot be detected, source and target branches match, the task worktree has uncommitted edits, or conflicts occur.
- Records task status as `merged` after successful merge execution.
- Records task status as `merge_conflict` when merge execution reaches a conflict.
- Supports `raptor task abandon <task-id>` to mark an unmerged task as `abandoned` without deleting metadata or worktrees.
- Supports `raptor task cleanup <task-id>` for validated cleanup of `merged`, `completed`, or `abandoned` task worktrees only.
- Does not launch autonomous agents, execute task-provided commands, install dependencies, call provider APIs, or push remotes.

Known gap: if metadata persistence fails before a task record exists, manual cleanup may still be required because cleanup is task-metadata driven.

## Implemented Swarm Planning Controls

`raptor swarm "description"` currently applies these planning controls:

- Requires a non-empty task description.
- Generates a deterministic safe task id from the description.
- Creates only the four v0.1 role worktrees under `.raptor/worktrees/<task-id>/`.
- Uses fixed role templates from `templates/`.
- Calls only DeepSeek before v0.1.
- Sends only the task description and role template to DeepSeek.
- Does not include `.env` files, repository contents, secrets, task metadata, or worktree files in the prompt context.
- Writes only `RAPTOR_PLAN.md` inside each assigned role worktree.
- Uses create-new writes for plan files to avoid silently overwriting existing files.
- Records per-agent planning status and plan file paths in `.raptor/swarms/<task-id>.toml`.
- Continues to preserve worktrees and metadata if planning is blocked by a missing API key.
- Treats generated plans as untrusted text for developer review.
- Does not execute model output, run shell commands, apply patches, modify application source files, run tests, commit, merge, or push.

## Implemented Swarm Proposal Controls

`raptor swarm implement <task-id> <role>` currently applies these proposal controls:

- Treats task id and role as untrusted input.
- Allows only `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent`.
- Loads only validated `.raptor/swarms/<task-id>.toml` metadata.
- Validates the selected role exists in the swarm metadata.
- Validates the selected role worktree path under `.raptor/worktrees/<task-id>/<role>/`.
- Requires `DEEPSEEK_API_KEY`; if missing, it records blocked status and does not write a proposal.
- Reads only the selected role's `RAPTOR_PLAN.md`.
- Calls only DeepSeek before v0.1.
- Sends only the task description, selected role, and selected role plan to DeepSeek.
- Writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside the selected role worktree.
- Uses create-new writes for proposal files to avoid silently overwriting existing files.
- Records per-agent implementation status and proposal file path in `.raptor/swarms/<task-id>.toml`.
- Treats generated proposals and patch-like content as untrusted Markdown text for developer review.
- Does not execute model output, run shell commands, apply patches, modify application source files, run tests, commit, merge, or push.

## Implemented Swarm Review And Diff Controls

`raptor review <task-id> [role]` and `raptor diff <task-id> [role]` currently apply these swarm inspection controls:

- Treat task id and optional role as untrusted input.
- Allow only `frontend-agent`, `backend-agent`, `test-agent`, and `security-agent` when a role is provided.
- Load validated `.raptor/swarms/<task-id>.toml` metadata only after no single-task metadata exists.
- Validate each inspected role exists in the swarm metadata.
- Validate each inspected role worktree path under `.raptor/worktrees/<task-id>/<role>/`.
- Reject unsafe task ids, unsafe roles, path traversal, missing metadata, missing worktrees, and unsafe stored worktree paths.
- Use structured git `status --short`, `diff`, and `diff --cached` calls in the validated role worktree.
- Fan out across all four roles when no role is provided.
- Inspect only the selected role when a role is provided.
- List untracked files under changed files; standard git diff output does not include untracked file contents until they are added.
- Do not write files, update metadata, stage changes, commit, merge, delete worktrees, run tests, call models, execute model output, install dependencies, or push.

## Implemented Test Execution Controls

`raptor test <task-id>` currently applies these supervised test controls:

- Treats task id, stored metadata, stored worktree paths, and explicit command arguments as untrusted input.
- Loads single-task metadata first; if none exists, loads swarm metadata.
- Validates single-task worktree paths against `.raptor/worktrees/<task-id>`.
- Validates swarm role worktree paths against `.raptor/worktrees/<task-id>/<role>/`.
- Rejects unsafe task ids, path traversal, missing metadata, missing worktrees, unsafe stored paths, invalid roles, unsafe programs, and unsafe arguments.
- Detects `cargo test` from `Cargo.toml`, `npm test` from `package.json` `scripts.test`, and `make test` from a `Makefile` `test` target.
- Allows explicit commands after `--` only for test-oriented subcommands such as `cargo test`, `npm test`, `pnpm test`, `yarn test`, `bun test`, `make test`, `go test`, `pytest`, `python -m pytest`, and `uv run pytest`.
- Runs commands with structured process arguments and sets the current directory to the validated worktree.
- Captures stdout, stderr, exit code, command source, target label, and pass/fail status.
- Does not use a shell, run commands from model output, install dependencies automatically, call models, update metadata, edit application source files, stage, commit, merge, delete worktrees, or push.

## Merge Controls

`raptor merge <task-id>` performs preflight checks and prints a dry-run summary only. `raptor merge <task-id> --execute` performs the same preflight checks, prints the exact confirmation phrase, reads typed confirmation from stdin, and then runs a supervised local git merge from the task branch into the current target branch.

Current preflight and merge execution controls:

- Treat task ids, task metadata, and stored worktree paths as untrusted input.
- Reject path traversal, unsafe task ids, and worktree paths outside `.raptor/worktrees/<task-id>`.
- Refuse to proceed if task metadata or the task worktree is missing.
- Refuse to proceed if the main repository has uncommitted changes.
- Refuse to proceed when unresolved conflicts are already present.
- Refuse to execute when the source or target branch cannot be detected.
- Refuse to execute when the source branch equals the target branch.
- Refuse to execute when task worktree changes are uncommitted, because V1 execution merges committed branch changes only.
- Require the developer to review `raptor review <task-id>` and/or `raptor diff <task-id>` output before merge confirmation.
- Print a dry-run summary before any mutation.
- Require typed confirmation exactly matching `MERGE <task-id>`.
- Abort on missing or mismatched confirmation.
- Update task metadata after merge success or merge conflict.
- Never execute task-provided commands, generated scripts, or dependency installation steps during merge.
- Never push to a remote or delete the task worktree in the first merge version.
- Never trust generated files blindly; the developer remains responsible for code review before confirming merge.

## Cleanup Controls

`raptor task cleanup <task-id>` is a destructive filesystem operation limited to task worktrees.

Current cleanup controls:

- Treat task ids, task metadata, and stored worktree paths as untrusted input.
- Reject unsafe task ids and path traversal attempts.
- Load only `.raptor/tasks/<task-id>.toml`.
- Validate the stored worktree path under `.raptor/worktrees/<task-id>`.
- Refuse cleanup unless task status is `merged`, `completed`, or `abandoned`.
- Print a cleanup plan before deletion.
- Require typed confirmation exactly matching `CLEANUP <task-id>`.
- Abort on missing, empty, or mismatched confirmation.
- Delete only the associated validated worktree directory.
- Use filesystem APIs directly instead of shelling out to `rm`.
- Preserve task metadata, task branches, and remotes.
- Never execute task-provided commands, generated scripts, dependency installation steps, push operations, or AI model calls.

## Current Confirmation Flow

Merge execution is explicit. Default `raptor merge <task-id>` remains dry-run/preflight and never prompts.

Execution command:

```bash
raptor merge <task-id> --execute
```

Confirmation requirements:

- The command prints all preflight details before prompting.
- The command prints the exact required phrase `MERGE <task-id>`.
- The entered phrase must include the actual task id.
- Empty input aborts.
- Mismatched input aborts.
- Non-interactive terminals abort unless a future CI-safe flag is explicitly implemented.
- Confirmation must come from the interactive user prompt only.
- Confirmation must not come from task metadata, environment variables, generated files, or model output.
- The command must never proceed because an AI model produced the confirmation phrase.
- Exact confirmation proceeds to local git merge execution only after preflight passes.

Optional future automation command:

```bash
raptor merge <task-id> --execute --no-confirm
```

That mode is not a V1 goal. It should be reserved for CI or policy-approved automation after repository policy controls exist.

Auditability requirements:

- Output records task id, source worktree, target branch, changed files, and confirmation result.
- Task metadata records lifecycle transition timestamps for merge success, merge conflict, and abandonment.

## Current Status

The repository contains a Rust workspace with an implemented CLI router, base `raptor` startup screen, `raptor tui`, `raptor init`, `raptor doctor`, `raptor provider list`, `raptor chat`, `raptor swarm`, `raptor swarm implement <task-id> <role>`, `raptor swarm run "mission"`, `raptor agent list`, `raptor agent show <id>`, `raptor agent create`, `raptor agent policy`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor task abandon <id>`, `raptor task cleanup <id>`, `raptor review <task-id> [role]`, `raptor diff <task-id> [role]`, `raptor test <task-id>`, `raptor merge <task-id>` preflight dry run, and `raptor merge <task-id> --execute` supervised local merge execution. Security controls described here are implemented for startup key detection, read-only dashboard rendering, provider metadata inspection, DeepSeek chat, DeepSeek swarm planning artifacts, swarm implementation proposal artifacts, live swarm agentic execution with per-worktree tool policy, role-aware swarm review/diff inspection, supervised test execution, persisted read-only agent session metadata, permission policy display, task setup, task inspection, lifecycle updates, cleanup, and merge execution. They remain design requirements for future autonomous source modification workflows.
