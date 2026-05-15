use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub const PROJECT_DESCRIPTION: &str = "An open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.";
pub const PROJECT_NAME: &str = "RaptorCode";
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
    Planned,
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
            Self::Planned => "planned",
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
    fn rejects_empty_task_description() {
        assert_eq!(
            TaskId::from_description("   ").unwrap_err(),
            TaskError::EmptyDescription
        );
    }
}
