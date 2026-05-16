# Security Design

Date: 2026-05-16
Author: Tim
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Security Model

RaptorCode assumes AI coding agents can make incorrect or unsafe changes. The system must keep developers in control and make agent actions inspectable before they affect the main working tree or remote repository.

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

## Provider And Agent Foundation Controls

`raptor provider list`, `raptor agent list`, and `raptor agent show <id>` are inspection-only commands.

Current provider controls:

- Provider ids are validated before configuration is accepted.
- Provider metadata is built in and local.
- DeepSeek is listed first for future integration, with OpenAI/Codex, Claude, Gemini, and local model boundaries represented as future providers.
- `raptor.toml` supports provider enablement metadata only.
- No API keys, tokens, secrets, endpoints, or credentials are stored in `raptor.toml`.
- No external requests, model calls, streaming connections, or provider SDKs are implemented.
- Completion execution returns an explicit disabled/not-implemented error.

Current agent controls:

- Agent ids are validated before lookup.
- Agent sessions are local-only and read-only in this foundation.
- `raptor agent list` starts from empty local session state.
- `raptor agent show <id>` validates the id and returns a clear not-found error when no session exists.
- Read-only agent execution mode does not allow file modification, shell execution, or model calls.
- Agent session models support future audit entries, but no autonomous coding loop exists.

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

The repository contains a Rust workspace with an implemented CLI router, `raptor init`, `raptor doctor`, `raptor provider list`, `raptor agent list`, `raptor agent show <id>`, `raptor task "description"`, `raptor task list`, `raptor task show <id>`, `raptor task abandon <id>`, `raptor task cleanup <id>`, `raptor review <task-id>`, `raptor diff <task-id>`, `raptor merge <task-id>` preflight dry run, and `raptor merge <task-id> --execute` supervised local merge execution. Security controls described here are implemented for provider metadata inspection, read-only agent session inspection, task setup, task inspection, lifecycle updates, cleanup, and merge execution. They remain design requirements for future model execution, autonomous agent, and TUI workflows.
