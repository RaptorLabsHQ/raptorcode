//! Presentation layer: `render_*` functions that turn command results and
//! domain types into the strings printed to the terminal, plus the shared
//! number/path/diff formatting utilities they (and the `task_commands` renderers)
//! build on. Extracted from `lib.rs` so formatting lives behind one boundary.

use std::path::{Path, PathBuf};

use raptor_agent::PermissionPolicy;
use raptor_core::{project_name, version, TaskStatus};
use raptor_git::{inspect_repository, inspect_repository_root};
use raptor_providers::{CompletionResponse, ProviderSummary, TokenUsage};
use raptor_store::StoredAgentSession;
use raptor_tools::command_version;

pub fn render_doctor(current_dir: &Path) -> String {
    let repository = inspect_repository(current_dir);
    let tools = [
        command_version("git", &["--version"]),
        command_version("cargo", &["--version"]),
        command_version("rustc", &["--version"]),
    ];

    let mut output = String::new();
    output.push_str(project_name());
    output.push_str(" doctor\n");
    output.push_str(&format!("version: {}\n", version()));
    output.push_str(&format!("current_dir: {}\n", current_dir.display()));
    output.push_str(&format!(
        "repository: {}\n",
        if repository.inside_work_tree {
            "detected"
        } else {
            "not detected"
        }
    ));

    if let Some(branch) = repository.branch.as_deref() {
        output.push_str(&format!("branch: {branch}\n"));
    }

    if let Some(git_dir) = repository.git_dir.as_deref() {
        output.push_str(&format!("git_dir: {git_dir}\n"));
    }

    for tool in tools {
        output.push_str(&format!(
            "tool.{}: {} ({})\n",
            tool.name,
            if tool.available {
                "available"
            } else {
                "unavailable"
            },
            tool.detail
        ));
    }

    output
}

pub(crate) fn project_config_dir(current_dir: &Path) -> PathBuf {
    inspect_repository_root(current_dir)
        .map(|repository_root| repository_root.path().to_path_buf())
        .unwrap_or_else(|_| current_dir.to_path_buf())
}

pub(crate) fn render_provider_list(config_status: &str, providers: &[ProviderSummary]) -> String {
    let mut output = format!("Provider configuration: {config_status}\n");

    if providers.is_empty() {
        output.push_str("No RaptorCode providers registered.\n");
        output.push_str("No external requests, model calls, or API integrations are enabled.\n");
        return output;
    }

    output.push_str("Provider | Name | Status | Registered | Default model | Capabilities\n");
    output.push_str("--- | --- | --- | --- | --- | ---\n");

    for provider in providers {
        let status = if provider.enabled {
            "enabled"
        } else if provider.configured {
            "disabled"
        } else {
            "not configured"
        };
        let default_model = provider
            .default_model
            .as_ref()
            .map(|model| model.as_str())
            .unwrap_or("-");
        let capabilities = provider.capabilities.labels();
        let capabilities = if capabilities.is_empty() {
            "-".to_owned()
        } else {
            capabilities.join(", ")
        };

        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            provider.provider_id.as_str(),
            provider.display_name,
            status,
            if provider.registered { "yes" } else { "no" },
            default_model,
            capabilities
        ));
    }

    output.push_str("Execution: disabled; provider list performs no network or model calls.\n");
    output
}

pub(crate) fn render_agent_created(stored: &StoredAgentSession, metadata_path: &Path) -> String {
    let session = &stored.session;

    format!(
        "Agent session created\nAgent: {}\nRole: {}\nState: {}\nExecution mode: {}\nProvider: {}\nModel: {}\nTask: {}\nMetadata: {}\nModel execution: not performed\nShell execution: not performed\nSource file writes: not performed\n",
        session.id.as_str(),
        session.role.as_str(),
        session.state.as_str(),
        session.execution_mode.as_str(),
        display_optional(session.provider_id.as_deref()),
        display_optional(session.model_id.as_deref()),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.task_id.as_str())
            .unwrap_or("-"),
        metadata_path.display()
    )
}

pub(crate) fn render_agent_list(sessions: &[StoredAgentSession]) -> String {
    if sessions.is_empty() {
        return "No RaptorCode agent sessions found.\nExecution: read-only foundation only; no model calls, shell execution, or file modification are configured.\n".to_owned();
    }

    let mut output = String::from(
        "ID | Role | State | Execution mode | Created | Updated | Provider | Model | Task\n",
    );
    output.push_str("--- | --- | --- | --- | --- | --- | --- | --- | ---\n");

    for stored in sessions {
        let session = &stored.session;
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            session.id.as_str(),
            session.role.as_str(),
            session.state.as_str(),
            session.execution_mode.as_str(),
            display_optional(session.created_at.as_deref()),
            display_optional(session.updated_at.as_deref()),
            display_optional(session.provider_id.as_deref()),
            display_optional(session.model_id.as_deref()),
            session
                .task_binding
                .as_ref()
                .map(|binding| binding.task_id.as_str())
                .unwrap_or("-")
        ));
    }

    output
}

pub(crate) fn render_agent_session(stored: &StoredAgentSession) -> String {
    let session = &stored.session;

    format!(
        "Agent: {}\nRole: {}\nState: {}\nExecution mode: {}\nCreated: {}\nUpdated: {}\nProvider: {}\nModel: {}\nTask: {}\nWorktree: {}\nMessages: {}\nEvents: {}\nAudit entries: {}\nAudit summary: {}\nMetadata: {}\nModel execution: not performed\nShell execution: not performed\nSource file writes: not performed\n",
        session.id.as_str(),
        session.role.as_str(),
        session.state.as_str(),
        session.execution_mode.as_str(),
        display_optional(session.created_at.as_deref()),
        display_optional(session.updated_at.as_deref()),
        display_optional(session.provider_id.as_deref()),
        display_optional(session.model_id.as_deref()),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.task_id.as_str())
            .unwrap_or("-"),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.worktree_path.display().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        session.messages.len(),
        session.events.len(),
        session.audit_entries.len(),
        display_optional(session.audit_summary.as_deref()),
        stored.metadata_path.display()
    )
}

pub(crate) fn render_permission_policy(policy: &PermissionPolicy) -> String {
    format!(
        "RaptorCode default permission policy\nApproval mode: {}\nFile read: {}\nFile write: {}\nShell execution: {}\nNetwork: {}\nModel execution: {}\nDangerous commands: {}\nFile write allowed: {}\nShell execution allowed: {}\nNetwork allowed: {}\nModel execution allowed: {}\nScope: this is the declared default for stored agent records. A live raptor session enforces a separate runtime boundary — workspace-scoped file writes (confirmation required to write outside the workspace) plus a destructive-command deny-list backstop — not these blanket denials.\nExecution: policy display only; no model calls, shell commands, network requests, or source file writes are performed.\n",
        policy.approval_mode.as_str(),
        policy.file_read.as_str(),
        policy.file_write.as_str(),
        policy.shell_execution.as_str(),
        policy.network.as_str(),
        policy.model_execution.as_str(),
        policy.dangerous_commands.as_str(),
        if policy.allows_file_write() { "yes" } else { "no" },
        if policy.allows_shell_execution() { "yes" } else { "no" },
        if policy.allows_network() { "yes" } else { "no" },
        if policy.allows_model_execution() { "yes" } else { "no" }
    )
}

pub(crate) fn render_chat_response(response: &CompletionResponse) -> String {
    let mut output = format!(
        "DeepSeek response\nProvider: {}\nModel: {}\nFinish reason: {}\nUsage: {}\n\n{}\n",
        response.provider_id.as_str(),
        response.model_id.as_str(),
        display_optional(response.finish_reason.as_deref()),
        render_token_usage(response.usage.as_ref()),
        response.content
    );

    output.push_str("Model output execution: not performed\n");
    output.push_str("Source file writes: not performed\n");
    output.push_str("Shell execution: not performed\n");
    output
}

pub(crate) fn render_token_usage(usage: Option<&TokenUsage>) -> String {
    let Some(usage) = usage else {
        return "-".to_owned();
    };

    format!(
        "prompt={} completion={} cache_hit={} cache_miss={} total={}",
        display_number_optional(usage.prompt_tokens),
        display_number_optional(usage.completion_tokens),
        display_number_optional(usage.prompt_cache_hit_tokens),
        display_number_optional(usage.prompt_cache_miss_tokens),
        display_number_optional(usage.total_tokens)
    )
}

/// Friendly, non-alarming context-usage line for interactive mode. Reframes the
/// raw DeepSeek prefix-cache accounting as a positive efficiency signal: prompt
/// caching is automatic and prefix-based, so the "miss" tokens are simply the new
/// tokens added this turn — never a failure. We surface the context size and the
/// cached share instead. Returns an empty string when there is nothing useful.
pub(crate) fn render_context_usage(usage: &TokenUsage) -> String {
    let Some(context_tokens) = usage.prompt_tokens else {
        return String::new();
    };

    match (usage.prompt_cache_hit_tokens, context_tokens) {
        (Some(hit), total) if total > 0 => {
            // Round to the nearest percent so e.g. 89.97% reads as 90%.
            let cached_percent = ((hit as u64 * 100 + total as u64 / 2) / total as u64).min(100);
            format!(
                "Context: {} tokens · {}% cached",
                group_thousands(total),
                cached_percent
            )
        }
        _ => format!("Context: {} tokens", group_thousands(context_tokens)),
    }
}

/// Formats an integer with thousands separators (e.g. 4268 → "4,268").
pub(crate) fn group_thousands(value: u32) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    let len = digits.len();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (len - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

pub(crate) fn push_diff_section(output: &mut String, diff: &str) {
    if diff.trim().is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    output.push_str(diff);
    if !diff.ends_with('\n') {
        output.push('\n');
    }
}

pub(crate) fn push_changed_files_section(output: &mut String, changed_files: &[String]) {
    if changed_files.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for file in changed_files {
        output.push_str("  ");
        output.push_str(file);
        output.push('\n');
    }
}

pub(crate) fn push_command_output_section(output: &mut String, command_output: &str) {
    if command_output.trim().is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for line in command_output.lines() {
        output.push_str("  ");
        output.push_str(line);
        output.push('\n');
    }
}

pub(crate) fn display_optional(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

pub(crate) fn display_path_optional(value: Option<&std::path::PathBuf>) -> String {
    value
        .map(|path| path.display().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_owned())
}

pub(crate) fn display_number(value: u32) -> String {
    value.to_string()
}

pub(crate) fn display_number_optional(value: Option<u32>) -> String {
    value.map(display_number).unwrap_or_else(|| "-".to_owned())
}

pub(crate) fn display_exit_code(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "terminated_by_signal".to_owned())
}

pub(crate) fn task_status_allows_cleanup(status: Option<TaskStatus>) -> bool {
    matches!(status, Some(TaskStatus::Merged | TaskStatus::Abandoned))
}

pub(crate) fn strip_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}
