use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub const CRATE_PURPOSE: &str = "Agent lifecycle primitives for RaptorCode.";
pub const BACKEND_AGENT_TEMPLATE: &str = include_str!("../../../templates/backend-agent.md");
pub const FRONTEND_AGENT_TEMPLATE: &str = include_str!("../../../templates/frontend-agent.md");
pub const SECURITY_AGENT_TEMPLATE: &str = include_str!("../../../templates/security-agent.md");
pub const TEST_AGENT_TEMPLATE: &str = include_str!("../../../templates/test-agent.md");

pub const BACKEND_AGENT_ROLE: &str = "backend-agent";
pub const FRONTEND_AGENT_ROLE: &str = "frontend-agent";
pub const SECURITY_AGENT_ROLE: &str = "security-agent";
pub const TEST_AGENT_ROLE: &str = "test-agent";

const SWARM_PLANNING_FOOTER: &str = "Return a concise Markdown implementation plan. Planning only. Do not modify files. Do not execute commands. Do not include autonomous coding loops, policy enforcement hooks, additional providers, cloud features, desktop UI, MCP, plugin systems, or any Phase 2 feature.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePromptTemplate {
    pub content: &'static str,
    pub file_name: &'static str,
    pub role: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Architect,
    Reviewer,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionState {
    Created,
    Waiting,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentExecutionMode {
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTaskBinding {
    pub task_id: String,
    pub worktree_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessage {
    pub content: String,
    pub role: AgentMessageRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageRole {
    Assistant,
    System,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub detail: String,
    pub kind: AgentEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEventKind {
    Created,
    StateChanged,
    Audit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAuditEntry {
    pub action: String,
    pub allowed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub audit_entries: Vec<AgentAuditEntry>,
    pub audit_summary: Option<String>,
    pub created_at: Option<String>,
    pub events: Vec<AgentEvent>,
    pub execution_mode: AgentExecutionMode,
    pub id: AgentId,
    pub messages: Vec<AgentMessage>,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub role: AgentRole,
    pub state: AgentSessionState,
    pub task_binding: Option<AgentTaskBinding>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionRegistry {
    sessions: Vec<AgentSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    InvalidAgentId(String),
    InvalidMetadata(String),
    InvalidModelId(String),
    InvalidProviderId(String),
    InvalidTaskId(String),
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicy {
    pub approval_mode: ApprovalMode,
    pub dangerous_commands: DangerousCommandPolicy,
    pub file_read: FileReadPolicy,
    pub file_write: FileWritePolicy,
    pub model_execution: ModelExecutionPolicy,
    pub network: NetworkPolicy,
    pub shell_execution: ShellExecutionPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DangerousCommandPolicy {
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileReadPolicy {
    RepositoryRootOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileWritePolicy {
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelExecutionPolicy {
    DenyUntilConfiguredAndExplicit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellExecutionPolicy {
    Deny,
}

pub fn v1_swarm_role_templates() -> &'static [RolePromptTemplate] {
    const TEMPLATES: [RolePromptTemplate; 4] = [
        RolePromptTemplate {
            content: FRONTEND_AGENT_TEMPLATE,
            file_name: "templates/frontend-agent.md",
            role: FRONTEND_AGENT_ROLE,
        },
        RolePromptTemplate {
            content: BACKEND_AGENT_TEMPLATE,
            file_name: "templates/backend-agent.md",
            role: BACKEND_AGENT_ROLE,
        },
        RolePromptTemplate {
            content: TEST_AGENT_TEMPLATE,
            file_name: "templates/test-agent.md",
            role: TEST_AGENT_ROLE,
        },
        RolePromptTemplate {
            content: SECURITY_AGENT_TEMPLATE,
            file_name: "templates/security-agent.md",
            role: SECURITY_AGENT_ROLE,
        },
    ];

    &TEMPLATES
}

pub fn role_prompt_template(role: &str) -> Result<RolePromptTemplate, AgentError> {
    v1_swarm_role_templates()
        .iter()
        .copied()
        .find(|template| template.role == role)
        .ok_or_else(|| AgentError::InvalidMetadata(format!("unknown swarm agent role: {role}")))
}

pub fn build_role_planning_prompt(
    task_id: &str,
    task_description: &str,
    role: &str,
) -> Result<String, AgentError> {
    validate_task_id(task_id)?;

    let template = role_prompt_template(role)?;
    let description = task_description.trim();

    if description.is_empty() {
        return Err(AgentError::InvalidMetadata(
            "task description is required".to_owned(),
        ));
    }

    Ok(format!(
        "RaptorCode v0.1 swarm planning request\n\nTask id: {task_id}\nTask description:\n{description}\n\nAgent role: {role}\nTemplate source: {}\n\nRole template:\n{}\n\n{SWARM_PLANNING_FOOTER}\n",
        template.file_name, template.content
    ))
}

impl AgentId {
    pub fn new(value: impl Into<String>) -> Result<Self, AgentError> {
        let value = value.into();
        let valid = !value.is_empty()
            && !value.starts_with('-')
            && !value.ends_with('-')
            && !value.contains("..")
            && value.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            });

        if valid {
            Ok(Self(value))
        } else {
            Err(AgentError::InvalidAgentId(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_session_parts(
        role: AgentRole,
        task_id: Option<&str>,
        provider_id: Option<&str>,
        model_id: Option<&str>,
    ) -> Result<Self, AgentError> {
        if let Some(task_id) = task_id {
            validate_task_id(task_id)?;
        }
        if let Some(provider_id) = provider_id {
            validate_provider_id(provider_id)?;
        }
        if let Some(model_id) = model_id {
            validate_model_id(model_id)?;
        }

        let seed = format!(
            "{}|{}|{}|{}",
            role.as_str(),
            task_id.unwrap_or("-"),
            provider_id.unwrap_or("-"),
            model_id.unwrap_or("-")
        );

        Self::new(format!(
            "agent-{}-{}",
            role.as_str(),
            stable_hash_hex(&seed)
        ))
    }
}

impl AgentRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Architect => "architect",
            Self::Reviewer => "reviewer",
            Self::Worker => "worker",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AgentError> {
        match value {
            "architect" => Ok(Self::Architect),
            "reviewer" => Ok(Self::Reviewer),
            "worker" => Ok(Self::Worker),
            _ => Err(AgentError::InvalidMetadata(format!(
                "unknown agent role: {value}"
            ))),
        }
    }
}

impl AgentSessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Waiting => "waiting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AgentError> {
        match value {
            "created" => Ok(Self::Created),
            "waiting" => Ok(Self::Waiting),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(AgentError::InvalidMetadata(format!(
                "unknown agent state: {value}"
            ))),
        }
    }
}

impl AgentExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AgentError> {
        match value {
            "read_only" => Ok(Self::ReadOnly),
            _ => Err(AgentError::InvalidMetadata(format!(
                "unknown execution mode: {value}"
            ))),
        }
    }

    pub fn allows_file_modification(&self) -> bool {
        false
    }

    pub fn allows_model_calls(&self) -> bool {
        false
    }

    pub fn allows_shell_execution(&self) -> bool {
        false
    }
}

impl AgentMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            role: AgentMessageRole::User,
        }
    }
}

impl AgentSession {
    pub fn new_read_only(id: AgentId, role: AgentRole) -> Self {
        Self {
            audit_entries: Vec::new(),
            audit_summary: None,
            created_at: None,
            events: vec![AgentEvent {
                detail: "session created".to_owned(),
                kind: AgentEventKind::Created,
            }],
            execution_mode: AgentExecutionMode::ReadOnly,
            id,
            messages: Vec::new(),
            model_id: None,
            provider_id: None,
            role,
            state: AgentSessionState::Created,
            task_binding: None,
            updated_at: None,
        }
    }

    pub fn transition_to(&mut self, state: AgentSessionState) {
        self.state = state;
        self.events.push(AgentEvent {
            detail: format!("state changed to {}", state.as_str()),
            kind: AgentEventKind::StateChanged,
        });
    }

    pub fn bind_task(
        &mut self,
        task_id: impl Into<String>,
        worktree_path: PathBuf,
    ) -> Result<(), AgentError> {
        let task_id = task_id.into();
        validate_task_id(&task_id)?;

        self.task_binding = Some(AgentTaskBinding {
            task_id,
            worktree_path,
        });

        Ok(())
    }

    pub fn set_provider_id(&mut self, provider_id: impl Into<String>) -> Result<(), AgentError> {
        let provider_id = provider_id.into();
        validate_provider_id(&provider_id)?;

        self.provider_id = Some(provider_id);
        Ok(())
    }

    pub fn set_model_id(&mut self, model_id: impl Into<String>) -> Result<(), AgentError> {
        let model_id = model_id.into();
        validate_model_id(&model_id)?;

        self.model_id = Some(model_id);
        Ok(())
    }

    pub fn to_metadata(&self) -> String {
        let mut output = String::new();

        push_key_value(&mut output, "id", self.id.as_str());
        push_key_value(&mut output, "role", self.role.as_str());
        push_key_value(&mut output, "state", self.state.as_str());
        push_key_value(&mut output, "execution_mode", self.execution_mode.as_str());
        if let Some(created_at) = self.created_at.as_deref() {
            push_key_value(&mut output, "created_at", created_at);
        }
        if let Some(updated_at) = self.updated_at.as_deref() {
            push_key_value(&mut output, "updated_at", updated_at);
        }
        if let Some(audit_summary) = self.audit_summary.as_deref() {
            push_key_value(&mut output, "audit_summary", audit_summary);
        }
        push_key_value(
            &mut output,
            "audit_entry_count",
            &self.audit_entries.len().to_string(),
        );

        if let Some(provider_id) = self.provider_id.as_deref() {
            push_key_value(&mut output, "provider_id", provider_id);
        }
        if let Some(model_id) = self.model_id.as_deref() {
            push_key_value(&mut output, "model_id", model_id);
        }
        if let Some(binding) = self.task_binding.as_ref() {
            push_key_value(&mut output, "task_id", &binding.task_id);
            push_key_value(
                &mut output,
                "worktree_path",
                &binding.worktree_path.display().to_string(),
            );
        }

        output
    }

    pub fn from_metadata(content: &str) -> Result<Self, AgentError> {
        let mut id = None;
        let mut role = None;
        let mut state = None;
        let mut execution_mode = None;
        let mut provider_id = None;
        let mut model_id = None;
        let mut task_id = None;
        let mut worktree_path = None;
        let mut created_at = None;
        let mut updated_at = None;
        let mut audit_summary = None;

        for (line_index, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (key, value) = trimmed.split_once('=').ok_or_else(|| {
                AgentError::InvalidMetadata(format!(
                    "line {} is not a key/value pair",
                    line_index + 1
                ))
            })?;
            let key = key.trim();
            let value = parse_metadata_value(line_index + 1, value.trim())?;

            match key {
                "id" => id = Some(AgentId::new(value)?),
                "role" => role = Some(AgentRole::parse(&value)?),
                "state" => state = Some(AgentSessionState::parse(&value)?),
                "execution_mode" => execution_mode = Some(AgentExecutionMode::parse(&value)?),
                "provider_id" => {
                    validate_provider_id(&value)?;
                    provider_id = Some(value);
                }
                "model_id" => {
                    validate_model_id(&value)?;
                    model_id = Some(value);
                }
                "task_id" => {
                    validate_task_id(&value)?;
                    task_id = Some(value);
                }
                "worktree_path" => worktree_path = Some(PathBuf::from(value)),
                "created_at" => created_at = Some(value),
                "updated_at" => updated_at = Some(value),
                "audit_summary" => audit_summary = Some(value),
                "audit_entry_count" => {}
                _ => {}
            }
        }

        let id = id.ok_or_else(|| AgentError::InvalidMetadata("missing agent id".to_owned()))?;
        let role = role.unwrap_or(AgentRole::Worker);
        let mut session = Self::new_read_only(id, role);
        session.state = state.unwrap_or(AgentSessionState::Created);
        session.execution_mode = execution_mode.unwrap_or(AgentExecutionMode::ReadOnly);
        session.provider_id = provider_id;
        session.model_id = model_id;
        session.created_at = created_at;
        session.updated_at = updated_at;
        session.audit_summary = audit_summary;

        if let (Some(task_id), Some(worktree_path)) = (task_id, worktree_path) {
            session.bind_task(task_id, worktree_path)?;
        }

        Ok(session)
    }
}

impl AgentSessionRegistry {
    pub fn empty() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    pub fn new(sessions: Vec<AgentSession>) -> Self {
        Self { sessions }
    }

    pub fn sessions(&self) -> &[AgentSession] {
        &self.sessions
    }

    pub fn find(&self, id: &AgentId) -> Result<&AgentSession, AgentError> {
        self.sessions
            .iter()
            .find(|session| session.id == *id)
            .ok_or_else(|| AgentError::NotFound(id.as_str().to_owned()))
    }
}

impl Display for AgentError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAgentId(agent_id) => write!(formatter, "agent id is not safe: {agent_id}"),
            Self::InvalidMetadata(message) => {
                write!(formatter, "agent metadata is invalid: {message}")
            }
            Self::InvalidModelId(model_id) => write!(formatter, "model id is not safe: {model_id}"),
            Self::InvalidProviderId(provider_id) => {
                write!(formatter, "provider id is not safe: {provider_id}")
            }
            Self::InvalidTaskId(task_id) => write!(formatter, "task id is not safe: {task_id}"),
            Self::NotFound(agent_id) => write!(formatter, "agent session not found: {agent_id}"),
        }
    }
}

impl Error for AgentError {}

impl PermissionPolicy {
    pub fn deny_by_default() -> Self {
        Self {
            approval_mode: ApprovalMode::Manual,
            dangerous_commands: DangerousCommandPolicy::Deny,
            file_read: FileReadPolicy::RepositoryRootOnly,
            file_write: FileWritePolicy::Deny,
            model_execution: ModelExecutionPolicy::DenyUntilConfiguredAndExplicit,
            network: NetworkPolicy::Deny,
            shell_execution: ShellExecutionPolicy::Deny,
        }
    }

    pub fn allows_file_write(&self) -> bool {
        false
    }

    pub fn allows_shell_execution(&self) -> bool {
        false
    }

    pub fn allows_network(&self) -> bool {
        false
    }

    pub fn allows_model_execution(&self) -> bool {
        false
    }
}

impl ApprovalMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
        }
    }
}

impl DangerousCommandPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

impl FileReadPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::RepositoryRootOnly => "repository_root_only",
        }
    }
}

impl FileWritePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

impl ModelExecutionPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DenyUntilConfiguredAndExplicit => "deny_until_configured_and_explicit",
        }
    }
}

impl NetworkPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

impl ShellExecutionPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Deny => "deny",
        }
    }
}

fn validate_task_id(task_id: &str) -> Result<(), AgentError> {
    let valid = !task_id.is_empty()
        && !task_id.starts_with('-')
        && !task_id.ends_with('-')
        && !task_id.contains("..")
        && task_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });

    if valid {
        Ok(())
    } else {
        Err(AgentError::InvalidTaskId(task_id.to_owned()))
    }
}

fn validate_provider_id(provider_id: &str) -> Result<(), AgentError> {
    let valid = !provider_id.is_empty()
        && !provider_id.starts_with('-')
        && !provider_id.ends_with('-')
        && !provider_id.contains("..")
        && provider_id.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });

    if valid {
        Ok(())
    } else {
        Err(AgentError::InvalidProviderId(provider_id.to_owned()))
    }
}

fn validate_model_id(model_id: &str) -> Result<(), AgentError> {
    let valid = !model_id.is_empty()
        && !model_id.starts_with('-')
        && !model_id.ends_with('-')
        && !model_id.contains("..")
        && model_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | '/')
        });

    if valid {
        Ok(())
    } else {
        Err(AgentError::InvalidModelId(model_id.to_owned()))
    }
}

fn stable_hash_hex(input: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;

    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }

    format!("{hash:016x}")
}

fn push_key_value(output: &mut String, key: &str, value: &str) {
    output.push_str(&format!("{key} = \"{}\"\n", escape_metadata_value(value)));
}

fn escape_metadata_value(value: &str) -> String {
    let mut escaped = String::new();

    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character => escaped.push(character),
        }
    }

    escaped
}

fn parse_metadata_value(line_number: usize, value: &str) -> Result<String, AgentError> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(AgentError::InvalidMetadata(format!(
            "line {line_number} value is not a quoted string"
        )));
    }

    unescape_metadata_value(line_number, &value[1..value.len() - 1])
}

fn unescape_metadata_value(line_number: usize, value: &str) -> Result<String, AgentError> {
    let mut unescaped = String::new();
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }

        let Some(escaped) = characters.next() else {
            return Err(AgentError::InvalidMetadata(format!(
                "line {line_number} has an incomplete escape sequence"
            )));
        };

        match escaped {
            '\\' => unescaped.push('\\'),
            '"' => unescaped.push('"'),
            'n' => unescaped.push('\n'),
            'r' => unescaped.push('\r'),
            't' => unescaped.push('\t'),
            _ => {
                return Err(AgentError::InvalidMetadata(format!(
                    "line {line_number} has an unsupported escape sequence"
                )));
            }
        }
    }

    Ok(unescaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_safe_agent_ids() {
        assert!(AgentId::new("agent-1").is_ok());
        assert!(AgentId::new("Agent-1").is_err());
        assert!(AgentId::new("../agent").is_err());
        assert!(AgentId::new("-agent").is_err());
    }

    #[test]
    fn loads_v1_swarm_role_templates() {
        let templates = v1_swarm_role_templates();

        assert_eq!(templates.len(), 4);
        assert!(templates
            .iter()
            .any(|template| template.role == FRONTEND_AGENT_ROLE
                && template.file_name == "templates/frontend-agent.md"
                && template.content.contains("UI and authentication flow")));
        assert!(templates
            .iter()
            .any(|template| template.role == BACKEND_AGENT_ROLE
                && template
                    .content
                    .contains("Environment variable considerations")));
        assert!(templates
            .iter()
            .any(|template| template.role == TEST_AGENT_ROLE
                && template.content.contains("Failure path tests")));
        assert!(templates
            .iter()
            .any(|template| template.role == SECURITY_AGENT_ROLE
                && template.content.contains("Security checklist")));
    }

    #[test]
    fn builds_role_planning_prompt_per_role() {
        let prompt = build_role_planning_prompt(
            "add-github-oauth-123",
            "Add GitHub OAuth to this Next.js app",
            SECURITY_AGENT_ROLE,
        )
        .unwrap();

        assert!(prompt.contains("RaptorCode v0.1 swarm planning request"));
        assert!(prompt.contains("Task id: add-github-oauth-123"));
        assert!(prompt.contains("Agent role: security-agent"));
        assert!(prompt.contains("templates/security-agent.md"));
        assert!(prompt.contains("Authentication and authorization risks"));
        assert!(prompt.contains("Planning only"));
    }

    #[test]
    fn role_planning_prompt_rejects_unsafe_input() {
        assert!(build_role_planning_prompt("../bad", "Task", FRONTEND_AGENT_ROLE).is_err());
        assert!(build_role_planning_prompt("safe-task", "   ", FRONTEND_AGENT_ROLE).is_err());
        assert!(build_role_planning_prompt("safe-task", "Task", "unknown-agent").is_err());
    }

    #[test]
    fn empty_agent_registry_lists_no_sessions() {
        let registry = AgentSessionRegistry::empty();

        assert!(registry.sessions().is_empty());
    }

    #[test]
    fn missing_agent_session_is_explicit() {
        let registry = AgentSessionRegistry::empty();
        let agent_id = AgentId::new("missing-agent").unwrap();

        let error = registry.find(&agent_id).unwrap_err();

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn read_only_execution_mode_blocks_execution_scaffolding() {
        let mode = AgentExecutionMode::ReadOnly;

        assert!(!mode.allows_file_modification());
        assert!(!mode.allows_shell_execution());
        assert!(!mode.allows_model_calls());
    }

    #[test]
    fn default_permission_policy_denies_mutating_execution() {
        let policy = PermissionPolicy::deny_by_default();

        assert_eq!(policy.file_read, FileReadPolicy::RepositoryRootOnly);
        assert!(!policy.allows_file_write());
        assert!(!policy.allows_shell_execution());
        assert!(!policy.allows_network());
        assert!(!policy.allows_model_execution());
        assert_eq!(policy.approval_mode.as_str(), "manual");
    }

    #[test]
    fn creates_deterministic_agent_ids_from_session_parts() {
        let first = AgentId::from_session_parts(
            AgentRole::Worker,
            Some("task-1"),
            Some("deepseek"),
            Some("deepseek-v4-pro"),
        )
        .unwrap();
        let second = AgentId::from_session_parts(
            AgentRole::Worker,
            Some("task-1"),
            Some("deepseek"),
            Some("deepseek-v4-pro"),
        )
        .unwrap();

        assert_eq!(first, second);
        assert!(first.as_str().starts_with("agent-worker-"));
    }

    #[test]
    fn session_lifecycle_records_state_events() {
        let mut session =
            AgentSession::new_read_only(AgentId::new("agent-1").unwrap(), AgentRole::Worker);

        session.transition_to(AgentSessionState::Waiting);

        assert_eq!(session.state, AgentSessionState::Waiting);
        assert_eq!(session.events.len(), 2);
        assert_eq!(session.events[1].kind, AgentEventKind::StateChanged);
    }

    #[test]
    fn agent_session_metadata_round_trips_core_fields() {
        let mut session =
            AgentSession::new_read_only(AgentId::new("agent-1").unwrap(), AgentRole::Reviewer);
        session.transition_to(AgentSessionState::Waiting);
        session.set_provider_id("deepseek").unwrap();
        session.set_model_id("deepseek-v4-pro").unwrap();
        session
            .bind_task("task-1", PathBuf::from("/tmp/task-1"))
            .unwrap();
        session.created_at = Some("1".to_owned());
        session.updated_at = Some("2".to_owned());
        session.audit_summary = Some("0 audit entries".to_owned());

        let parsed = AgentSession::from_metadata(&session.to_metadata()).unwrap();

        assert_eq!(parsed.id.as_str(), "agent-1");
        assert_eq!(parsed.role, AgentRole::Reviewer);
        assert_eq!(parsed.state, AgentSessionState::Waiting);
        assert_eq!(parsed.execution_mode, AgentExecutionMode::ReadOnly);
        assert_eq!(parsed.provider_id.as_deref(), Some("deepseek"));
        assert_eq!(parsed.model_id.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(parsed.task_binding.as_ref().unwrap().task_id, "task-1");
        assert_eq!(parsed.created_at.as_deref(), Some("1"));
        assert_eq!(parsed.updated_at.as_deref(), Some("2"));
        assert_eq!(parsed.audit_summary.as_deref(), Some("0 audit entries"));
    }

    #[test]
    fn rejects_unsafe_session_metadata_ids() {
        assert!(AgentSession::from_metadata("id = \"../agent\"\n").is_err());
        assert!(AgentSession::from_metadata("id = \"agent-1\"\ntask_id = \"../task\"\n").is_err());
        assert!(AgentSession::from_metadata("id = \"agent-1\"\nprovider_id = \"Bad\"\n").is_err());
        assert!(
            AgentSession::from_metadata("id = \"agent-1\"\nmodel_id = \"../model\"\n").is_err()
        );
    }
}
