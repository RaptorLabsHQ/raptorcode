//! Task metadata store: `.raptor/tasks/<task-id>.toml`, one file per task,
//! created once and rewritten in place on each status transition.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use raptor_core::{is_safe_hyphen_name, TaskId, TaskMetadata, TaskStatus};

use crate::atomic_io::{atomic_create, atomic_write};
use crate::paths::{validate_base_dir, validate_child_path};
use crate::toml_codec::{
    id_from_metadata_path, parse_quoted_value, push_toml_path, push_toml_string,
};
use crate::{current_unix_timestamp, RAPTOR_STATE_DIR, TASKS_DIR};

#[derive(Debug)]
pub enum TaskStoreError {
    AlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
    NotFound(String),
    Parse { path: PathBuf, message: String },
    UnsafeBasePath(PathBuf),
    UnsafeTaskId(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatusTransition {
    Abandoned,
    MergeConflict,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTaskMetadata {
    pub abandoned_at: Option<String>,
    pub branch: Option<String>,
    pub conflict_at: Option<String>,
    pub created_at: Option<String>,
    pub description: Option<String>,
    pub id: String,
    pub metadata_path: PathBuf,
    pub merged_at: Option<String>,
    pub repository_root: Option<PathBuf>,
    pub status: Option<TaskStatus>,
    pub updated_at: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

impl TaskStatusTransition {
    pub fn as_str(&self) -> &'static str {
        self.status().as_str()
    }

    /// The lifecycle status this transition moves the task into.
    pub fn status(self) -> TaskStatus {
        match self {
            Self::Abandoned => TaskStatus::Abandoned,
            Self::MergeConflict => TaskStatus::MergeConflict,
            Self::Merged => TaskStatus::Merged,
        }
    }

    fn timestamp_key(&self) -> &'static str {
        match self {
            Self::Abandoned => "abandoned_at",
            Self::MergeConflict => "conflict_at",
            Self::Merged => "merged_at",
        }
    }
}

impl Display for TaskStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "task metadata already exists: {}",
                    path.display()
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::NotFound(task_id) => {
                write!(formatter, "task metadata not found: {task_id}")
            }
            Self::Parse { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
            Self::UnsafeTaskId(task_id) => {
                write!(formatter, "task id is not safe: {task_id}")
            }
        }
    }
}

impl Error for TaskStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::AlreadyExists(_)
            | Self::NotFound(_)
            | Self::Parse { .. }
            | Self::UnsafeBasePath(_)
            | Self::UnsafeTaskId(_) => None,
        }
    }
}

pub fn persist_task_metadata(
    repository_root: &Path,
    metadata: &TaskMetadata,
) -> Result<PathBuf, TaskStoreError> {
    validate_base_dir(repository_root).map_err(TaskStoreError::UnsafeBasePath)?;

    let task_dir = repository_root.join(RAPTOR_STATE_DIR).join(TASKS_DIR);
    validate_child_path(repository_root, &task_dir).map_err(TaskStoreError::UnsafeBasePath)?;

    fs::create_dir_all(&task_dir).map_err(|source| TaskStoreError::Io {
        path: task_dir.clone(),
        source,
    })?;

    let metadata_path = task_metadata_path(repository_root, &metadata.id)?;

    atomic_create(&metadata_path, &render_task_metadata(metadata)).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            TaskStoreError::AlreadyExists(metadata_path.clone())
        } else {
            TaskStoreError::Io {
                path: metadata_path.clone(),
                source,
            }
        }
    })?;

    Ok(metadata_path)
}

pub fn task_metadata_exists(
    repository_root: &Path,
    task_id: &TaskId,
) -> Result<bool, TaskStoreError> {
    Ok(task_metadata_path(repository_root, task_id)?.exists())
}

pub fn task_metadata_path(
    repository_root: &Path,
    task_id: &TaskId,
) -> Result<PathBuf, TaskStoreError> {
    task_metadata_path_for_id(repository_root, task_id.as_str())
}

pub fn list_task_metadata(
    repository_root: &Path,
) -> Result<Vec<StoredTaskMetadata>, TaskStoreError> {
    validate_base_dir(repository_root).map_err(TaskStoreError::UnsafeBasePath)?;

    let task_dir = task_directory_path(repository_root)?;

    match fs::symlink_metadata(&task_dir) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(TaskStoreError::UnsafeBasePath(task_dir)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(TaskStoreError::Io {
                path: task_dir,
                source,
            });
        }
    }

    let mut tasks = Vec::new();

    for entry in fs::read_dir(&task_dir).map_err(|source| TaskStoreError::Io {
        path: task_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| TaskStoreError::Io {
            path: task_dir.clone(),
            source,
        })?;
        let path = entry.path();
        let Some(task_id) = id_from_metadata_path(&path) else {
            continue;
        };

        if validate_task_id(&task_id).is_err() {
            continue;
        }

        // Skip an unreadable/torn file rather than failing the whole listing.
        if let Ok(metadata) = read_task_metadata(repository_root, &task_id) {
            tasks.push(metadata);
        }
    }

    tasks.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(tasks)
}

pub fn read_task_metadata(
    repository_root: &Path,
    task_id: &str,
) -> Result<StoredTaskMetadata, TaskStoreError> {
    validate_task_id(task_id)?;

    let task_dir = task_directory_path(repository_root)?;
    let metadata_path = task_metadata_path_for_id(repository_root, task_id)?;

    let file_metadata = match fs::symlink_metadata(&metadata_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(TaskStoreError::NotFound(task_id.to_owned()));
        }
        Err(source) => {
            return Err(TaskStoreError::Io {
                path: metadata_path,
                source,
            });
        }
    };

    if !file_metadata.file_type().is_file() {
        return Err(TaskStoreError::NotFound(task_id.to_owned()));
    }

    let canonical_task_dir = task_dir
        .canonicalize()
        .map_err(|source| TaskStoreError::Io {
            path: task_dir.clone(),
            source,
        })?;
    let canonical_metadata_path =
        metadata_path
            .canonicalize()
            .map_err(|source| TaskStoreError::Io {
                path: metadata_path.clone(),
                source,
            })?;

    if !canonical_metadata_path.starts_with(canonical_task_dir) {
        return Err(TaskStoreError::UnsafeBasePath(metadata_path));
    }

    let content = fs::read_to_string(&metadata_path).map_err(|source| TaskStoreError::Io {
        path: metadata_path.clone(),
        source,
    })?;

    parse_task_metadata(&metadata_path, task_id, &content)
}

pub fn update_task_status(
    repository_root: &Path,
    task_id: &str,
    transition: TaskStatusTransition,
) -> Result<StoredTaskMetadata, TaskStoreError> {
    let mut task = read_task_metadata(repository_root, task_id)?;
    let timestamp = current_unix_timestamp();

    task.status = Some(transition.status());
    task.updated_at = Some(timestamp.clone());

    match transition.timestamp_key() {
        "abandoned_at" => task.abandoned_at = Some(timestamp),
        "conflict_at" => task.conflict_at = Some(timestamp),
        "merged_at" => task.merged_at = Some(timestamp),
        _ => {}
    }

    write_stored_task_metadata(&task)?;

    read_task_metadata(repository_root, task_id)
}

fn task_metadata_path_for_id(
    repository_root: &Path,
    task_id: &str,
) -> Result<PathBuf, TaskStoreError> {
    validate_base_dir(repository_root).map_err(TaskStoreError::UnsafeBasePath)?;
    validate_task_id(task_id)?;

    let metadata_path = task_directory_path(repository_root)?.join(format!("{task_id}.toml"));
    validate_child_path(repository_root, &metadata_path).map_err(TaskStoreError::UnsafeBasePath)?;

    Ok(metadata_path)
}

fn task_directory_path(repository_root: &Path) -> Result<PathBuf, TaskStoreError> {
    validate_base_dir(repository_root).map_err(TaskStoreError::UnsafeBasePath)?;

    let task_dir = repository_root.join(RAPTOR_STATE_DIR).join(TASKS_DIR);
    validate_child_path(repository_root, &task_dir).map_err(TaskStoreError::UnsafeBasePath)?;

    Ok(task_dir)
}

fn validate_task_id(task_id: &str) -> Result<(), TaskStoreError> {
    if is_safe_hyphen_name(task_id) {
        Ok(())
    } else {
        Err(TaskStoreError::UnsafeTaskId(task_id.to_owned()))
    }
}

fn render_task_metadata(metadata: &TaskMetadata) -> String {
    let timestamp = current_unix_timestamp();
    let task = StoredTaskMetadata {
        abandoned_at: None,
        branch: Some(metadata.worktree.branch_name.clone()),
        conflict_at: None,
        created_at: Some(timestamp.clone()),
        description: Some(metadata.description.clone()),
        id: metadata.id.as_str().to_owned(),
        metadata_path: PathBuf::new(),
        merged_at: None,
        repository_root: Some(metadata.worktree.repository_root.clone()),
        status: Some(metadata.status),
        updated_at: Some(timestamp),
        worktree_path: Some(metadata.worktree.path.clone()),
    };

    render_stored_task_metadata(&task)
}

fn parse_task_metadata(
    metadata_path: &Path,
    fallback_task_id: &str,
    content: &str,
) -> Result<StoredTaskMetadata, TaskStoreError> {
    let mut parsed = StoredTaskMetadata {
        abandoned_at: None,
        branch: None,
        conflict_at: None,
        created_at: None,
        description: None,
        id: fallback_task_id.to_owned(),
        metadata_path: metadata_path.to_path_buf(),
        merged_at: None,
        repository_root: None,
        status: None,
        updated_at: None,
        worktree_path: None,
    };

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| TaskStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message: format!("line {} is not a key/value pair", line_index + 1),
            })?;
        let key = key.trim();
        let value = parse_quoted_value(line_index + 1, value.trim()).map_err(|message| {
            TaskStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message,
            }
        })?;

        match key {
            "id" => {
                validate_task_id(&value)?;

                if value != fallback_task_id {
                    return Err(TaskStoreError::Parse {
                        path: metadata_path.to_path_buf(),
                        message: "metadata id does not match file name".to_owned(),
                    });
                }

                parsed.id = value;
            }
            "description" => parsed.description = Some(value),
            "status" => {
                parsed.status =
                    Some(
                        TaskStatus::parse(&value).ok_or_else(|| TaskStoreError::Parse {
                            path: metadata_path.to_path_buf(),
                            message: format!(
                                "line {} has an unknown task status: {value}",
                                line_index + 1
                            ),
                        })?,
                    )
            }
            "branch" => parsed.branch = Some(value),
            "abandoned_at" => parsed.abandoned_at = Some(value),
            "conflict_at" => parsed.conflict_at = Some(value),
            "created" | "created_at" => parsed.created_at = Some(value),
            "merged_at" => parsed.merged_at = Some(value),
            "repository_root" => parsed.repository_root = Some(PathBuf::from(value)),
            "updated" | "updated_at" => parsed.updated_at = Some(value),
            "worktree_path" => parsed.worktree_path = Some(PathBuf::from(value)),
            _ => {}
        }
    }

    Ok(parsed)
}

fn write_stored_task_metadata(task: &StoredTaskMetadata) -> Result<(), TaskStoreError> {
    atomic_write(&task.metadata_path, &render_stored_task_metadata(task)).map_err(|source| {
        TaskStoreError::Io {
            path: task.metadata_path.clone(),
            source,
        }
    })
}

fn render_stored_task_metadata(task: &StoredTaskMetadata) -> String {
    let mut output = String::new();

    push_toml_string(&mut output, "id", Some(&task.id));
    push_toml_string(&mut output, "description", task.description.as_deref());
    push_toml_string(&mut output, "status", task.status.map(TaskStatus::as_str));
    push_toml_string(&mut output, "branch", task.branch.as_deref());
    push_toml_path(
        &mut output,
        "repository_root",
        task.repository_root.as_ref(),
    );
    push_toml_path(&mut output, "worktree_path", task.worktree_path.as_ref());
    push_toml_string(&mut output, "created_at", task.created_at.as_deref());
    push_toml_string(&mut output, "updated_at", task.updated_at.as_deref());
    push_toml_string(&mut output, "merged_at", task.merged_at.as_deref());
    push_toml_string(&mut output, "conflict_at", task.conflict_at.as_deref());
    push_toml_string(&mut output, "abandoned_at", task.abandoned_at.as_deref());

    output
}
