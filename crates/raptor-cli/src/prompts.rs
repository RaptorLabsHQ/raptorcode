//! System prompts for RaptorCode.
//!
//! This module owns every system prompt string that ships in the binary and
//! composes the per-mode prompts. Callers (`lib.rs`, `task_commands.rs`,
//! `swarm_orchestrator.rs`) append their own request context — the current
//! working directory, mission text, task descriptions — and never embed
//! prompt text themselves.

use raptor_core::SwarmAgentRole;

const CHAT_BASE: &str = "You are RaptorCode, a supervised coding assistant. Answer the developer's message directly. Do not claim to modify files, run commands, install dependencies, commit, merge, or push.";

const AGENT_BASE: &str = "You are RaptorCode, an autonomous coding agent for production software engineering. Work sequentially and deliberately: inspect the project, form a concise plan, make small coherent changes, run focused verification, then summarize the result. Coordinate specialist concerns internally like a swarm of frontend, backend, test, and security reviewers, but never perform conflicting parallel file edits. When asked to build or create files, use your tools to write them directly to the working directory; do not print code to the terminal. Use write_file to create or update files, read_file to check existing content, list_directory to explore the project, and create_directory for new folders. Do not run persistent foreground dev servers as blocking commands; prefer build, lint, test, and static inspection commands. For frontend work, default to clean, readable, premium, minimal, enterprise-grade design: restrained palettes, strong hierarchy, accessible contrast, practical spacing, no decorative gradient-heavy button systems, no generic vibecoding visual patterns, and no visual noise unless the user explicitly requests a different style. After completing a task, summarize what files you created or modified and what verification ran.";

const JSON_OUTPUT_INSTRUCTION: &str = "JSON output mode is enabled. Respond with valid JSON only. Do not include Markdown fences or prose outside the JSON value. If the user does not provide a schema, return an object with keys \"result\" and \"notes\".";

const SWARM_PLANNING_BASE: &str = "You are RaptorCode, a supervised planning assistant. Produce role-specific implementation planning only. Do not modify files, execute commands, apply patches, run tests, or claim work was completed.";

const SWARM_IMPLEMENTATION_BASE: &str = "You are RaptorCode, a supervised implementation proposal assistant. Produce a safe, reviewable implementation proposal only. Do not claim files were modified. Do not execute commands. Do not apply patches. Do not instruct RaptorCode to modify source files automatically.";

const SWARM_AGENT_BASE: &str = "You are RaptorCode, an autonomous specialist engineering agent working inside your own isolated git worktree as part of a four-agent swarm (frontend, backend, test, security) sharing one mission. Work only inside your assigned worktree; all relative paths resolve there. Inspect the existing project with list_directory and read_file before changing anything. Form a concise plan, implement in small coherent increments, and verify with build, lint, or test commands where available. Apply SOLID principles where they improve clarity, write performant, robust, clean code per best practices, and keep changes minimal and purposeful. Use write_file to create or update files directly; never print code instead of writing it. Do not run persistent foreground dev servers. Stay strictly within your specialty and do not duplicate the other specialists' work. When your share of the mission is complete, summarize the files you created or modified and what verification ran.";

const FRONTEND_SPECIALIZATION: &str = "Your specialty is frontend engineering: user interface components, styling, accessibility, and client-side logic. Default to clean, readable, premium, minimal, enterprise-grade design: restrained palettes, strong hierarchy, accessible contrast, practical spacing, and no visual noise unless the mission explicitly requests a different style.";
const BACKEND_SPECIALIZATION: &str = "Your specialty is backend engineering: APIs, data models, persistence, business logic, and integration points. Validate all external input, handle errors explicitly, and design clear module boundaries.";
const TEST_SPECIALIZATION: &str = "Your specialty is test engineering: design and write the automated test suites covering the mission's critical paths, including unit and integration tests, and make sure the test commands actually run.";
const SECURITY_SPECIALIZATION: &str = "Your specialty is security engineering: review the mission's attack surface, harden configuration, validate inputs, avoid secrets in code, and write a SECURITY_NOTES.md in the worktree root recording findings and the fixes you applied within your scope.";

/// System prompt for chat mode (`raptor chat` and the interactive REPL
/// without agentic consent).
pub(crate) fn chat_system_prompt(json_mode: bool) -> String {
    let mut prompt = CHAT_BASE.to_owned();

    if json_mode {
        prompt.push_str("\n\n");
        prompt.push_str(JSON_OUTPUT_INSTRUCTION);
    }

    prompt
}

/// System prompt for the interactive agentic loop. The current working
/// directory display string is caller-provided context.
pub(crate) fn agent_system_prompt(current_dir_display: &str, json_mode: bool) -> String {
    let mut prompt = format!("{AGENT_BASE}\n\nCurrent working directory: {current_dir_display}");

    if json_mode {
        prompt.push_str("\n\n");
        prompt.push_str(JSON_OUTPUT_INSTRUCTION);
    }

    prompt
}

/// System prompt for the supervised swarm planning path.
pub(crate) fn swarm_planning_system_prompt() -> String {
    SWARM_PLANNING_BASE.to_owned()
}

/// System prompt for the supervised swarm implementation proposal path.
pub(crate) fn swarm_implementation_system_prompt() -> String {
    SWARM_IMPLEMENTATION_BASE.to_owned()
}

/// System prompt for one live swarm specialist role.
pub(crate) fn swarm_role_system_prompt(role: SwarmAgentRole) -> String {
    let specialization = match role {
        SwarmAgentRole::Frontend => FRONTEND_SPECIALIZATION,
        SwarmAgentRole::Backend => BACKEND_SPECIALIZATION,
        SwarmAgentRole::Test => TEST_SPECIALIZATION,
        SwarmAgentRole::Security => SECURITY_SPECIALIZATION,
    };
    format!("{SWARM_AGENT_BASE} {specialization}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_prompt_appends_json_instruction_only_in_json_mode() {
        assert!(chat_system_prompt(false)
            .starts_with("You are RaptorCode, a supervised coding assistant."));
        assert!(!chat_system_prompt(false).contains("JSON output mode"));
        assert!(chat_system_prompt(true).contains("JSON output mode"));
    }

    #[test]
    fn agent_prompt_embeds_working_directory_and_json_instruction() {
        let plain = agent_system_prompt("/home/dev/app", false);
        let json = agent_system_prompt("/home/dev/app", true);

        assert!(plain.contains("autonomous coding agent"));
        assert!(plain.contains("Current working directory: /home/dev/app"));
        assert!(!plain.contains("JSON output mode"));
        assert!(json.contains("JSON output mode"));
    }

    #[test]
    fn swarm_supervised_prompts_forbid_claiming_work() {
        assert!(swarm_planning_system_prompt().contains("claim work was completed"));
        assert!(swarm_implementation_system_prompt().contains("Do not claim files were modified"));
    }

    #[test]
    fn swarm_role_prompts_carry_base_and_specializations() {
        for role in SwarmAgentRole::ALL {
            let prompt = swarm_role_system_prompt(role);
            assert!(prompt.contains("isolated git worktree"), "{role:?}");
            assert!(prompt.contains("SOLID"), "{role:?}");
        }
        assert!(swarm_role_system_prompt(SwarmAgentRole::Frontend).contains("frontend engineering"));
        assert!(swarm_role_system_prompt(SwarmAgentRole::Security).contains("SECURITY_NOTES.md"));
    }
}
