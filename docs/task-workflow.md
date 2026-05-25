# Task Workflow

Date: 2026-05-18
Author: Bob Vasic
Company: RaptorLabs.dev

RaptorCode is an open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.

V1 scope is CLI + TUI + multi-agent orchestrator. V1 is not a full terminal emulator replacement.

## Summary

The first single-worktree workflow is `raptor task "description"`. The v0.1 multi-agent workflow is `raptor swarm "description"`. The swarm workflow creates role worktrees, can generate role-specific DeepSeek planning artifacts, can generate one supervised implementation proposal for a selected role, can inspect swarm role worktrees with read-only review and diff commands, and can run supervised test commands inside validated worktrees. It does not launch autonomous coding agents or modify application source code automatically.

## Command

Create task infrastructure:

```bash
raptor task "describe the supervised task"
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor swarm implement <task-id> frontend-agent
```

Inspect persisted task metadata:

```bash
raptor task "describe the supervised task"
raptor swarm "Add GitHub OAuth to this Next.js app"
raptor swarm implement <task-id> frontend-agent
raptor task list
raptor task show <id>
raptor task abandon <id>
raptor task cleanup <id>
raptor review <task-id>
raptor review <task-id> frontend-agent
raptor diff <task-id>
raptor diff <task-id> frontend-agent
raptor test <task-id>
raptor test <task-id> -- cargo test
raptor merge <task-id>
raptor merge <task-id> --execute
```

## Behavior

The command:

- Requires a non-empty task description.
- Creates a deterministic task id from the description.
- Detects the git repository root.
- Plans a branch named `raptor-task-<task-id>`.
- Creates an isolated git worktree under `.raptor/worktrees/<task-id>`.
- Persists task metadata under `.raptor/tasks/<task-id>.toml`.
- Reports task id, status, branch, worktree path, and metadata path.

`raptor swarm "description"`:

- Requires a non-empty task description.
- Creates a deterministic task id from the description.
- Detects the git repository root.
- Creates four role branches:
  - `raptor/<task-id>/frontend-agent`
  - `raptor/<task-id>/backend-agent`
  - `raptor/<task-id>/test-agent`
  - `raptor/<task-id>/security-agent`
- Creates four isolated role worktrees:
  - `.raptor/worktrees/<task-id>/frontend-agent`
  - `.raptor/worktrees/<task-id>/backend-agent`
  - `.raptor/worktrees/<task-id>/test-agent`
  - `.raptor/worktrees/<task-id>/security-agent`
- Persists swarm metadata under `.raptor/swarms/<task-id>.toml`.
- Records provider `deepseek`, model `deepseek-chat`, swarm planning status, per-agent planning status, and per-agent plan file paths when generated.
- Uses role templates from `templates/frontend-agent.md`, `templates/backend-agent.md`, `templates/test-agent.md`, and `templates/security-agent.md`.
- If `DEEPSEEK_API_KEY` is set, calls DeepSeek once per role and writes `RAPTOR_PLAN.md` inside each assigned role worktree.
- If `DEEPSEEK_API_KEY` is missing, keeps the worktrees and metadata, records `blocked_missing_deepseek_api_key`, and directs the developer to `https://platform.deepseek.com/api_keys`.
- Prints role, branch, worktree path, and status for each agent.
- Does not execute model output, apply patches, modify application source files, run tests, merge, push, or execute task-provided commands.

Each generated `RAPTOR_PLAN.md` includes the task id, task description, agent role, provider, model, generated plan, and a safety note that the AI-generated plan must be reviewed before implementation.

`raptor swarm implement <task-id> <role>`:

- Treats the task id and role as untrusted input.
- Allows only `frontend-agent`, `backend-agent`, `test-agent`, or `security-agent`.
- Loads existing swarm metadata from `.raptor/swarms/<task-id>.toml`.
- Validates that the selected role exists in the swarm.
- Validates that the selected role worktree is under `.raptor/worktrees/<task-id>/<role>/`.
- Requires `DEEPSEEK_API_KEY`; if it is missing, no proposal is written and the command prints `https://platform.deepseek.com/api_keys`.
- Reads only the selected role's `RAPTOR_PLAN.md`.
- Calls DeepSeek once for the selected role.
- Writes only `.raptor/worktrees/<task-id>/<role>/RAPTOR_IMPLEMENTATION_PROPOSAL.md`.
- Records per-agent `implementation_status`, `proposal_file_path`, provider, model, and `updated_at` in swarm metadata.
- Does not execute model output, apply patches, modify application source files, run tests, merge, push, or execute task-provided commands.

The generated implementation proposal preserves patch-like model output as reviewable Markdown text only. Applying source changes remains manual and supervised.

`raptor review <task-id> [role]` and `raptor diff <task-id> [role]` are read-only. They first load single-task metadata from `.raptor/tasks/` when it exists. If no single-task metadata exists, they load swarm metadata from `.raptor/swarms/<task-id>.toml` and inspect swarm role worktrees. Passing a role limits inspection to one of `frontend-agent`, `backend-agent`, `test-agent`, or `security-agent`; omitting the role fans out across all roles.

`raptor task list`:

- Reads task metadata from `.raptor/tasks/`.
- Prints task id, status, created timestamp when available, worktree path, and description.
- Handles an empty task directory gracefully.
- Does not modify files.

`raptor task show <id>`:

- Treats the task id as untrusted input.
- Rejects unsafe ids and path traversal attempts.
- Reads only `.raptor/tasks/<id>.toml`.
- Prints full task details.
- Does not modify files.

`raptor task abandon <id>`:

- Treats the task id as untrusted input.
- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Refuses already merged or completed tasks.
- Marks the task as `abandoned`.
- Records an `updated_at` timestamp and an `abandoned_at` timestamp.
- Does not delete metadata, delete the worktree, delete branches, push remotes, execute task metadata commands, or call AI models.

`raptor task cleanup <id>`:

- Treats the task id and stored worktree path as untrusted input.
- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Refuses cleanup unless task status is `merged`, `completed`, or `abandoned`.
- Prints a cleanup plan before deleting anything.
- Prints the exact required phrase `CLEANUP <task-id>`.
- Reads typed confirmation from stdin only.
- Deletes only the associated validated worktree directory after exact confirmation.
- Preserves task metadata, task branches, and remotes.
- Does not push, run tests, install dependencies, execute task-provided commands, or call AI models.

`raptor review <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Verifies the task worktree exists.
- Prints task id, status, description, worktree path, git status summary, and changed files.
- Does not stage, commit, merge, delete, or modify files.

If single-task metadata does not exist, `raptor review <task-id>`:

- Loads existing swarm metadata from `.raptor/swarms/<task-id>.toml`.
- Validates each role worktree path against `.raptor/worktrees/<task-id>/<role>/`.
- Prints swarm task id, description, status, planning status, each inspected role, role statuses, worktree paths, plan/proposal paths, git status summaries, and changed files.
- Does not write metadata, stage, commit, merge, delete, run tests, call models, execute model output, or modify files.

`raptor review <task-id> <role>`:

- Allows only the four v0.1 roles.
- Loads swarm metadata and inspects only the selected role worktree.
- Rejects unsafe roles, unknown roles, missing metadata, missing worktrees, and unsafe worktree paths.

`raptor diff <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Prints changed files plus unstaged and staged git diffs for the task worktree.
- Does not stage, commit, merge, delete, or modify files.

If single-task metadata does not exist, `raptor diff <task-id>`:

- Loads existing swarm metadata from `.raptor/swarms/<task-id>.toml`.
- Validates each role worktree path against `.raptor/worktrees/<task-id>/<role>/`.
- Prints each inspected role, role worktree path, proposal path when recorded, changed files, unstaged diffs, and staged diffs.
- Lists untracked files under changed files because standard git diff output does not include untracked file contents until they are added.
- Does not write metadata, stage, commit, merge, delete, run tests, call models, execute model output, or modify files.

`raptor diff <task-id> <role>`:

- Allows only the four v0.1 roles.
- Loads swarm metadata and inspects only the selected role worktree.
- Rejects unsafe roles, unknown roles, missing metadata, missing worktrees, and unsafe worktree paths.

`raptor test <task-id>`:

- Treats the task id and stored worktree paths as untrusted input.
- Loads single-task metadata from `.raptor/tasks/<task-id>.toml` when present.
- If no single-task metadata exists, loads swarm metadata from `.raptor/swarms/<task-id>.toml`.
- Validates single-task worktrees under `.raptor/worktrees/<task-id>`.
- Validates swarm role worktrees under `.raptor/worktrees/<task-id>/<role>/`.
- Runs the selected test command inside each validated target worktree.
- Auto-detects `cargo test` from `Cargo.toml`, `npm test` from `package.json` `scripts.test`, and `make test` from a `Makefile` `test` target.
- Accepts explicit test commands after `--`, for example `raptor test <task-id> -- cargo test`.
- Allows only test-oriented commands and subcommands.
- Captures stdout, stderr, exit code, target label, worktree path, command source, and pass/fail status.
- Returns a failed CLI result when any target command fails.
- Does not install dependencies, call models, execute model output, update metadata, stage, commit, merge, delete, push, or modify application source files automatically.

`raptor merge <task-id>`:

- Loads existing task metadata from `.raptor/tasks/<task-id>.toml`.
- Validates the stored worktree path against `.raptor/worktrees/<task-id>`.
- Verifies the target repository is a git repository.
- Verifies the target repository worktree is clean.
- Verifies the task worktree exists and has changes.
- Rejects unresolved conflict states.
- Prints a dry-run summary with task metadata, target repository, target branch when detectable, worktree path, worktree branch when detectable, and changed files.
- Explicitly states that no merge was executed.
- Does not stage, commit, merge, push, delete worktrees, run tests, install dependencies, execute task-provided commands, or call AI models.

`raptor merge <task-id> --execute`:

- Runs the same preflight checks as `raptor merge <task-id>`.
- Prints the same dry-run summary before confirmation.
- Prints the exact required phrase `MERGE <task-id>`.
- Reads typed confirmation from stdin only.
- Aborts on empty input, mismatched input, or unavailable interactive confirmation.
- Requires detectable target and source branches.
- Rejects source branch equal to target branch.
- Rejects uncommitted task worktree changes because V1 merge execution merges committed branch changes only.
- Runs a supervised local `git merge` from the task worktree branch into the current target branch after exact confirmation.
- Prints final merge status and merged files.
- Records task status as `merged` after successful merge execution.
- Records task status as `merge_conflict` if merge execution reaches a conflict.
- Does not mark a task as merged when preflight fails.
- Does not push, delete worktrees, run tests, install dependencies, execute task-provided commands, or call AI models.

## Supervised Merge Workflow

This section defines the current dry-run behavior and supervised merge execution behavior.

### Scope

The first merge workflow must be supervised only.

- `raptor merge <task-id>` must never merge without explicit user confirmation.
- It must never run if the main repository has uncommitted changes unless a future force mode is explicitly designed and documented.
- It must never delete task worktrees in the first merge version.
- It must not push to a remote.
- It must not launch AI agents or run task-provided commands.

### Required Preconditions

The merge command stops before merge execution unless all of these checks pass:

- The task id is valid and path-safe.
- Task metadata exists under `.raptor/tasks/<task-id>.toml`.
- The metadata contains a worktree path.
- The task worktree path exists.
- The stored worktree path resolves safely under `.raptor/worktrees/<task-id>`.
- The main repository worktree is clean.
- The task worktree has changes to merge.
- The target branch is detectable.
- The source task worktree branch is detectable.
- The source branch is different from the target branch.
- The task worktree has no uncommitted edits during execution.
- No unresolved conflicts are already present in the main repository or task worktree.
- The developer has reviewed `raptor review <task-id>` and/or `raptor diff <task-id>` output before confirming merge.

### Explicit Non-Goals

The first merge version must not include:

- Auto-merge without confirmation.
- Force merge.
- Conflict auto-resolution.
- Worktree deletion.
- Remote push.
- AI agent execution.
- Automatic dependency installation.

### Current Command Behavior

`raptor merge <task-id>` currently performs dry-run preflight only:

1. Validate the task id.
2. Load task metadata from `.raptor/tasks/<task-id>.toml`.
3. Validate the stored worktree path against `.raptor/worktrees/<task-id>`.
4. Verify the main repository is clean.
5. Verify the task worktree exists and has changes.
6. Verify no unresolved conflicts are present.
7. Print a dry-run summary with:
   - task id
   - task status
   - task description
   - target repository path
   - target branch detected from the main repository
   - worktree branch detected from the task worktree
   - worktree path
   - changed file list
8. Print that no merge was executed.
9. Print the expected `--execute` behavior.

### Current Typed Confirmation Flow

The default `raptor merge <task-id>` behavior remains dry-run/preflight and does not prompt. Supervised merge execution uses:

```bash
raptor merge <task-id> --execute
```

The command prints all preflight details before asking for confirmation. After preflight passes, it prints the exact confirmation phrase:

```text
MERGE <task-id>
```

The user must enter that exact phrase before merge execution. For a concrete task id such as `fix-parser-123`, the required confirmation would be:

```text
MERGE fix-parser-123
```

Confirmation safety rules:

- Confirmation happens only after all preflight checks pass.
- The confirmation phrase includes the actual task id.
- Empty input aborts.
- Mismatched input aborts.
- A non-interactive terminal aborts unless a future CI-safe flag is explicitly implemented.
- Confirmation is read only from the interactive user prompt.
- Confirmation must not be read from task metadata, environment variables, generated files, or model output.
- The command must never proceed because an AI model generated the confirmation phrase.
- Exact confirmation starts local merge execution only after preflight has passed.

Optional future automation may use:

```bash
raptor merge <task-id> --execute --no-confirm
```

That mode is not a V1 goal. It should be allowed only for CI or policy-approved automation after a policy engine exists.

Merge execution output includes:

- final status after success, abort, or conflict
- auditable output recording task id, source worktree, target branch, changed files, and confirmation result
- task status update output when merge succeeds or when a merge conflict is recorded

### Operational Notes

- Clean merge: print the resulting target branch status, mark the task `merged`, and leave the task worktree intact.
- Conflict: stop immediately, mark the task `merge_conflict`, report conflicted files, do not auto-resolve, do not delete the worktree, and leave the repository state visible for developer recovery.
- Dirty main repository: abort before merge planning and instruct the developer to commit, stash, or discard local changes manually.
- Dirty task worktree: abort before merge execution because uncommitted task edits are not part of a branch merge.
- Missing task metadata: return a clear not-found error for the task id.
- Missing task worktree: return a clear worktree-not-found error and do not read arbitrary filesystem paths.
- No task changes: abort because there is nothing to merge.

## Task Lifecycle

Task metadata stores status values as plain strings for compatibility with the existing `.raptor/tasks/*.toml` format.

Current lifecycle statuses:

- `planned`: task infrastructure exists and is not merged or abandoned.
- `merged`: supervised merge execution completed successfully.
- `merge_conflict`: merge execution reached a conflict and stopped without auto-resolution.
- `abandoned`: a developer marked the task as intentionally abandoned.
- `completed`: accepted as a safe cleanup state for compatibility with future lifecycle naming.

Current timestamp fields:

- `created_at`: written when task metadata is first persisted.
- `updated_at`: written when task metadata is created or when lifecycle status changes.
- `merged_at`: written when status changes to `merged`.
- `conflict_at`: written when status changes to `merge_conflict`.
- `abandoned_at`: written when status changes to `abandoned`.

### Cleanup Workflow

`raptor task cleanup <task-id>` removes only the validated task worktree directory. It does not delete task metadata in V1, because metadata is the local audit record for what was created, merged, abandoned, or cleaned up.

Cleanup requires:

- Valid task id.
- Existing task metadata.
- Stored worktree path present in metadata.
- Stored worktree path validated under `.raptor/worktrees/<task-id>`.
- Existing worktree directory.
- Task status of `merged`, `completed`, or `abandoned`.
- Exact typed confirmation:

```text
CLEANUP <task-id>
```

Cleanup aborts on missing metadata, unsafe task id, unsafe stored worktree path, missing worktree, unsafe status, missing confirmation, empty confirmation, or mismatched confirmation.

Cleanup does not delete task metadata, delete branches, push remotes, run generated commands, install dependencies, execute task metadata commands, or call AI models.

### Security Notes

- Treat the task id as untrusted input.
- Treat stored task metadata and stored worktree paths as untrusted.
- Reject path traversal and unsafe names.
- Reject worktree paths outside `.raptor/worktrees/<task-id>`.
- Do not execute commands from task descriptions, generated files, or task metadata.
- Do not trust generated files blindly; require developer review before merge confirmation.
- Never push remotely.
- Never delete the task worktree.
- Never install dependencies.
- Never auto-resolve conflicts.
- Target branch must be detected and shown before confirmation.
- Source branch or source worktree must be detected and shown before confirmation.
- Dirty target repositories abort.
- Unresolved conflicts abort.

### Implementation Plan

The implementation should keep CLI orchestration thin and put git-specific checks in `raptor-git`.

Implemented preflight pieces:

- CLI parser accepts `raptor merge <task-id>`.
- CLI parser accepts `raptor merge <task-id> --execute`.
- CLI parser accepts `raptor task abandon <task-id>`.
- CLI parser accepts `raptor task cleanup <task-id>`.
- `raptor-store` loads the existing `.raptor/tasks/*.toml` metadata format.
- `raptor-store` updates task lifecycle status and transition timestamps.
- `raptor-git` validates stored worktree paths using the same safety boundary as `raptor review` and `raptor diff`.
- `raptor-git` plans and executes validated worktree cleanup with filesystem APIs instead of shelling out to `rm`.
- Read-only git preflight checks cover main repository cleanliness, task worktree status, changed files, and unresolved conflict detection.
- CLI confirmation flow prints the exact confirmation phrase and aborts on empty, mismatched, or unavailable confirmation input.
- `raptor-git` executes local merges from committed task branches after confirmation.
- Merge execution rejects missing branches, same source and target branch, dirty task worktrees, missing worktrees, unsafe worktree paths, and unresolved conflicts.

Future merge pieces:

- Add optional merge preflight and execution timestamps to future task metadata.

### Future Phases

- Cleanup status recording after validated worktree deletion.
- Optional branch cleanup command after explicit design.
- Merge conflict assistant that explains conflicts without applying automatic resolution.
- Automated test execution before merge.
- Policy engine for repository-specific merge rules.
- Signed commits.
- Branch protection awareness.

## Local State

`.raptor/` is ignored by git and is reserved for local runtime state.

Current layout:

```text
.raptor/
  swarms/
    <task-id>.toml
  tasks/
    <task-id>.toml
  worktrees/
    <single-task-id>/
    <swarm-task-id>/
      frontend-agent/
        RAPTOR_PLAN.md
        RAPTOR_IMPLEMENTATION_PROPOSAL.md
      backend-agent/
      test-agent/
      security-agent/
```

## Safety Rules

- Task descriptions are validated before task ids are created.
- Branch and worktree names must use safe lowercase ASCII, digits, and hyphens.
- Swarm branch names use safe slash-separated branch components: `raptor/<task-id>/<role>`.
- Parent-directory traversal is rejected.
- Worktree paths must stay inside the repository root.
- Existing worktree paths are not overwritten.
- Existing task metadata is not overwritten.
- Existing swarm metadata is not overwritten.
- Git commands are invoked through structured process arguments.
- Task inspection commands reject unsafe ids and avoid arbitrary file reads.
- Review and diff commands verify stored worktree paths before invoking read-only git inspection.
- Merge preflight verifies stored worktree paths before invoking read-only git status inspection.
- Test execution verifies stored worktree paths before invoking validated test commands.
- Cleanup verifies stored worktree paths before deleting and removes only the associated validated worktree directory.

## Current Limits

- No autonomous agents are launched.
- `raptor swarm` creates role worktrees, metadata, and role planning artifacts when `DEEPSEEK_API_KEY` is set.
- `raptor swarm` calls only DeepSeek before v0.1 and sends only the task description plus role template.
- `raptor swarm` writes only `RAPTOR_PLAN.md` inside assigned role worktrees.
- `raptor swarm implement` writes only `RAPTOR_IMPLEMENTATION_PROPOSAL.md` inside the selected role worktree.
- `raptor swarm` and `raptor swarm implement` do not modify application source files inside role worktrees.
- `raptor test <task-id>` runs detected or explicit test commands in validated task or swarm role worktrees.
- `raptor test <task-id>` does not install dependencies, call models, execute model output, update metadata, commit, merge, delete worktrees, or push.
- Task setup and inspection commands do not create commits.
- No remote pushes are performed.
- `raptor merge <task-id>` is preflight dry-run only.
- `raptor merge <task-id> --execute` may create a normal git merge commit when Git requires one.
- `raptor merge <task-id> --execute` does not merge uncommitted task worktree edits; commit task branch changes first.
- `raptor task cleanup <task-id>` deletes validated task worktree directories only for safe lifecycle states.
- Task cleanup does not delete metadata or branches in V1.
- Failed metadata persistence before a task record is available may require manual cleanup.

## v0.1 Scope Note

Task lifecycle cleanup refinements are not the next priority for v0.1 unless they directly block the launch demo. The active v0.1 path is:

```text
swarm -> DeepSeek -> worktrees -> tests -> diffs -> TUI -> release
```

Persisted agent sessions and permission policy were implemented early but are frozen until after v0.1.

## Recommended Next Step

PROMPT 25 - Final verification and tag v0.1.0
