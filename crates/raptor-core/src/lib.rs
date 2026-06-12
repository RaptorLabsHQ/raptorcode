use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub const PROJECT_DESCRIPTION: &str = "An open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.";
pub const PROJECT_NAME: &str = "RaptorCode";
pub const SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS: &str = "blocked_missing_deepseek_api_key";
pub const SWARM_IMPLEMENTATION_FAILED_STATUS: &str = "failed";
pub const SWARM_LIVE_RUN_ABORTED_STATUS: &str = "live_implementation_aborted";
pub const SWARM_LIVE_RUN_COMPLETED_STATUS: &str = "live_implementation_completed";
pub const SWARM_LIVE_RUN_FAILED_STATUS: &str = "live_implementation_failed";
pub const SWARM_LIVE_RUN_RUNNING_STATUS: &str = "live_implementation_running";
pub const SWARM_PENDING_MODEL_EXECUTION_STATUS: &str = "pending_model_execution";
pub const SWARM_PLANNING_COMPLETED_STATUS: &str = "planning_completed";
pub const SWARM_PLANNING_FAILED_STATUS: &str = "planning_failed";
pub const SWARM_PROPOSAL_GENERATED_STATUS: &str = "proposal_generated";
pub const V1_SCOPE: &str = "CLI + TUI + multi-agent orchestrator";

pub fn project_description() -> &'static str {
    PROJECT_DESCRIPTION
}

pub fn project_name() -> &'static str {
    PROJECT_NAME
}

pub fn v1_scope() -> &'static str {
    V1_SCOPE
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskId(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Abandoned,
    MergeConflict,
    Merged,
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmAgentRole {
    Frontend,
    Backend,
    Test,
    Security,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub branch_name: String,
    pub path: PathBuf,
    pub repository_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMetadata {
    pub description: String,
    pub id: TaskId,
    pub status: TaskStatus,
    pub worktree: WorktreeInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    EmptyDescription,
}

impl TaskId {
    pub fn from_description(description: &str) -> Result<Self, TaskError> {
        let trimmed = description.trim();

        if trimmed.is_empty() {
            return Err(TaskError::EmptyDescription);
        }

        Ok(Self(format!(
            "{}-{}",
            description_slug(trimmed),
            stable_hash_hex(trimmed)
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Abandoned => "abandoned",
            Self::MergeConflict => "merge_conflict",
            Self::Merged => "merged",
            Self::Planned => "planned",
        }
    }
}

impl SwarmAgentRole {
    pub const ALL: [Self; 4] = [Self::Frontend, Self::Backend, Self::Test, Self::Security];

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Frontend => "frontend-agent",
            Self::Backend => "backend-agent",
            Self::Test => "test-agent",
            Self::Security => "security-agent",
        }
    }
}

impl TaskMetadata {
    pub fn new(
        id: TaskId,
        description: impl Into<String>,
        status: TaskStatus,
        worktree: WorktreeInfo,
    ) -> Self {
        Self {
            description: description.into(),
            id,
            status,
            worktree,
        }
    }
}

impl Display for TaskError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDescription => formatter.write_str("task description is required"),
        }
    }
}

impl Error for TaskError {}

fn description_slug(description: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for character in description.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator && !slug.is_empty() {
            slug.push('-');
            previous_was_separator = true;
        }

        if slug.len() >= 48 {
            break;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug
    }
}

fn stable_hash_hex(input: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = FNV_OFFSET;

    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_project_metadata() {
        assert_eq!(project_name(), "RaptorCode");
        assert!(project_description().contains("multi-agent coding terminal"));
        assert_eq!(v1_scope(), "CLI + TUI + multi-agent orchestrator");
    }

    #[test]
    fn creates_deterministic_safe_task_id() {
        let first = TaskId::from_description("Fix parser edge cases").unwrap();
        let second = TaskId::from_description("Fix parser edge cases").unwrap();

        assert_eq!(first, second);
        assert!(first.as_str().starts_with("fix-parser-edge-cases-"));
        assert!(first
            .as_str()
            .chars()
            .all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'));
    }

    #[test]
    fn exposes_task_status_values() {
        assert_eq!(TaskStatus::Planned.as_str(), "planned");
        assert_eq!(TaskStatus::Merged.as_str(), "merged");
        assert_eq!(TaskStatus::MergeConflict.as_str(), "merge_conflict");
        assert_eq!(TaskStatus::Abandoned.as_str(), "abandoned");
    }

    #[test]
    fn exposes_v1_swarm_roles() {
        let roles = SwarmAgentRole::ALL.map(|role| role.as_str());

        assert_eq!(
            roles,
            [
                "frontend-agent",
                "backend-agent",
                "test-agent",
                "security-agent"
            ]
        );
        assert_eq!(
            SWARM_PENDING_MODEL_EXECUTION_STATUS,
            "pending_model_execution"
        );
        assert_eq!(
            SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS,
            "blocked_missing_deepseek_api_key"
        );
        assert_eq!(SWARM_PLANNING_COMPLETED_STATUS, "planning_completed");
        assert_eq!(SWARM_PLANNING_FAILED_STATUS, "planning_failed");
        assert_eq!(SWARM_PROPOSAL_GENERATED_STATUS, "proposal_generated");
        assert_eq!(SWARM_IMPLEMENTATION_FAILED_STATUS, "failed");
    }

    #[test]
    fn rejects_empty_task_description() {
        assert_eq!(
            TaskId::from_description("   ").unwrap_err(),
            TaskError::EmptyDescription
        );
    }
}
