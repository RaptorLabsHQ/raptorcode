use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub const CRATE_PURPOSE: &str = "Agent lifecycle primitives for RaptorCode.";

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
    pub events: Vec<AgentEvent>,
    pub execution_mode: AgentExecutionMode,
    pub id: AgentId,
    pub messages: Vec<AgentMessage>,
    pub model_id: Option<String>,
    pub provider_id: Option<String>,
    pub role: AgentRole,
    pub state: AgentSessionState,
    pub task_binding: Option<AgentTaskBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionRegistry {
    sessions: Vec<AgentSession>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentError {
    InvalidAgentId(String),
    InvalidMetadata(String),
    NotFound(String),
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
        }
    }

    pub fn transition_to(&mut self, state: AgentSessionState) {
        self.state = state;
        self.events.push(AgentEvent {
            detail: format!("state changed to {}", state.as_str()),
            kind: AgentEventKind::StateChanged,
        });
    }

    pub fn bind_task(&mut self, task_id: impl Into<String>, worktree_path: PathBuf) {
        self.task_binding = Some(AgentTaskBinding {
            task_id: task_id.into(),
            worktree_path,
        });
    }

    pub fn to_metadata(&self) -> String {
        let mut output = String::new();

        push_key_value(&mut output, "id", self.id.as_str());
        push_key_value(&mut output, "role", self.role.as_str());
        push_key_value(&mut output, "state", self.state.as_str());
        push_key_value(&mut output, "execution_mode", self.execution_mode.as_str());

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
                "provider_id" => provider_id = Some(value),
                "model_id" => model_id = Some(value),
                "task_id" => task_id = Some(value),
                "worktree_path" => worktree_path = Some(PathBuf::from(value)),
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

        if let (Some(task_id), Some(worktree_path)) = (task_id, worktree_path) {
            session.bind_task(task_id, worktree_path);
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
            Self::NotFound(agent_id) => write!(formatter, "agent session not found: {agent_id}"),
        }
    }
}

impl Error for AgentError {}

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
        session.provider_id = Some("deepseek".to_owned());
        session.model_id = Some("deepseek-chat".to_owned());
        session.bind_task("task-1", PathBuf::from("/tmp/task-1"));

        let parsed = AgentSession::from_metadata(&session.to_metadata()).unwrap();

        assert_eq!(parsed.id.as_str(), "agent-1");
        assert_eq!(parsed.role, AgentRole::Reviewer);
        assert_eq!(parsed.state, AgentSessionState::Waiting);
        assert_eq!(parsed.execution_mode, AgentExecutionMode::ReadOnly);
        assert_eq!(parsed.provider_id.as_deref(), Some("deepseek"));
        assert_eq!(parsed.model_id.as_deref(), Some("deepseek-chat"));
        assert_eq!(parsed.task_binding.as_ref().unwrap().task_id, "task-1");
    }
}
