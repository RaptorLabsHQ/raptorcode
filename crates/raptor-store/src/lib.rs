use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use raptor_core::{TaskId, TaskMetadata};

pub const CONFIG_FILE_NAME: &str = "raptor.toml";
pub const CRATE_PURPOSE: &str = "Local state storage boundaries for RaptorCode.";
pub const RAPTOR_STATE_DIR: &str = ".raptor";
pub const TASKS_DIR: &str = "tasks";

const DEFAULT_CONFIG: &str = "# RaptorCode project configuration\nversion = 1\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitConfigOutcome {
    Created(PathBuf),
    Exists(PathBuf),
}

#[derive(Debug)]
pub enum InitConfigError {
    Io { path: PathBuf, source: io::Error },
    NotDirectory(PathBuf),
    UnsafeBasePath(PathBuf),
}

#[derive(Debug)]
pub enum TaskStoreError {
    AlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
    UnsafeBasePath(PathBuf),
}

impl Display for InitConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::NotDirectory(path) => {
                write!(formatter, "{} is not a directory", path.display())
            }
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
        }
    }
}

impl Error for InitConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NotDirectory(_) | Self::UnsafeBasePath(_) => None,
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
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
        }
    }
}

impl Error for TaskStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::AlreadyExists(_) | Self::UnsafeBasePath(_) => None,
        }
    }
}

pub fn init_project_config(base_dir: &Path) -> Result<InitConfigOutcome, InitConfigError> {
    validate_base_dir(base_dir)?;

    let metadata = fs::metadata(base_dir).map_err(|source| InitConfigError::Io {
        path: base_dir.to_path_buf(),
        source,
    })?;

    if !metadata.is_dir() {
        return Err(InitConfigError::NotDirectory(base_dir.to_path_buf()));
    }

    let config_path = base_dir.join(CONFIG_FILE_NAME);

    if config_path.exists() {
        return Ok(InitConfigOutcome::Exists(config_path));
    }

    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Ok(InitConfigOutcome::Exists(config_path));
        }
        Err(source) => {
            return Err(InitConfigError::Io {
                path: config_path,
                source,
            });
        }
    };

    file.write_all(DEFAULT_CONFIG.as_bytes())
        .map_err(|source| InitConfigError::Io {
            path: config_path.clone(),
            source,
        })?;

    Ok(InitConfigOutcome::Created(config_path))
}

pub fn persist_task_metadata(
    repository_root: &Path,
    metadata: &TaskMetadata,
) -> Result<PathBuf, TaskStoreError> {
    validate_task_base_dir(repository_root)?;

    let task_dir = repository_root.join(RAPTOR_STATE_DIR).join(TASKS_DIR);
    validate_child_path(repository_root, &task_dir)?;

    fs::create_dir_all(&task_dir).map_err(|source| TaskStoreError::Io {
        path: task_dir.clone(),
        source,
    })?;

    let metadata_path = task_metadata_path(repository_root, &metadata.id)?;

    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&metadata_path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(TaskStoreError::AlreadyExists(metadata_path));
        }
        Err(source) => {
            return Err(TaskStoreError::Io {
                path: metadata_path,
                source,
            });
        }
    };

    file.write_all(render_task_metadata(metadata).as_bytes())
        .map_err(|source| TaskStoreError::Io {
            path: metadata_path.clone(),
            source,
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
    validate_task_base_dir(repository_root)?;

    let metadata_path = repository_root
        .join(RAPTOR_STATE_DIR)
        .join(TASKS_DIR)
        .join(format!("{}.toml", task_id.as_str()));
    validate_child_path(repository_root, &metadata_path)?;

    Ok(metadata_path)
}

fn validate_base_dir(base_dir: &Path) -> Result<(), InitConfigError> {
    if base_dir.as_os_str().is_empty() {
        return Err(InitConfigError::UnsafeBasePath(base_dir.to_path_buf()));
    }

    if base_dir
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(InitConfigError::UnsafeBasePath(base_dir.to_path_buf()));
    }

    Ok(())
}

fn validate_task_base_dir(base_dir: &Path) -> Result<(), TaskStoreError> {
    if base_dir.as_os_str().is_empty()
        || base_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(TaskStoreError::UnsafeBasePath(base_dir.to_path_buf()));
    }

    Ok(())
}

fn validate_child_path(base_dir: &Path, child_path: &Path) -> Result<(), TaskStoreError> {
    if child_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
        || !child_path.starts_with(base_dir)
    {
        return Err(TaskStoreError::UnsafeBasePath(child_path.to_path_buf()));
    }

    Ok(())
}

fn render_task_metadata(metadata: &TaskMetadata) -> String {
    format!(
        "id = \"{}\"\ndescription = \"{}\"\nstatus = \"{}\"\nbranch = \"{}\"\nrepository_root = \"{}\"\nworktree_path = \"{}\"\n",
        metadata.id.as_str(),
        escape_toml_string(&metadata.description),
        metadata.status.as_str(),
        escape_toml_string(&metadata.worktree.branch_name),
        escape_toml_string(&metadata.worktree.repository_root.display().to_string()),
        escape_toml_string(&metadata.worktree.path.display().to_string())
    )
}

fn escape_toml_string(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use raptor_core::{TaskMetadata, TaskStatus, WorktreeInfo};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn create(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "raptor-store-{name}-{}-{nonce}",
                std::process::id()
            ));

            fs::create_dir_all(&path).unwrap();

            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn init_creates_config_when_missing() {
        let test_dir = TestDir::create("create");
        let outcome = init_project_config(&test_dir.path).unwrap();
        let config_path = test_dir.path.join(CONFIG_FILE_NAME);

        assert_eq!(outcome, InitConfigOutcome::Created(config_path.clone()));
        assert_eq!(fs::read_to_string(config_path).unwrap(), DEFAULT_CONFIG);
    }

    #[test]
    fn init_refuses_to_overwrite_existing_config() {
        let test_dir = TestDir::create("exists");
        let config_path = test_dir.path.join(CONFIG_FILE_NAME);
        fs::write(&config_path, "custom = true\n").unwrap();

        let outcome = init_project_config(&test_dir.path).unwrap();

        assert_eq!(outcome, InitConfigOutcome::Exists(config_path.clone()));
        assert_eq!(fs::read_to_string(config_path).unwrap(), "custom = true\n");
    }

    #[test]
    fn init_rejects_parent_dir_components() {
        let error = init_project_config(Path::new("../unsafe")).unwrap_err();

        assert!(matches!(error, InitConfigError::UnsafeBasePath(_)));
    }

    #[test]
    fn persists_task_metadata_without_overwrite() {
        let test_dir = TestDir::create("task");
        let task_id = TaskId::from_description("Persist task metadata").unwrap();
        let metadata = TaskMetadata::new(
            task_id.clone(),
            "Persist task metadata",
            TaskStatus::Planned,
            WorktreeInfo {
                branch_name: format!("raptor-task-{}", task_id.as_str()),
                path: test_dir
                    .path
                    .join(".raptor")
                    .join("worktrees")
                    .join(task_id.as_str()),
                repository_root: test_dir.path.clone(),
            },
        );

        let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();

        assert!(metadata_path.exists());
        assert!(task_metadata_exists(&test_dir.path, &task_id).unwrap());
        assert!(matches!(
            persist_task_metadata(&test_dir.path, &metadata).unwrap_err(),
            TaskStoreError::AlreadyExists(_)
        ));
    }

    #[test]
    fn escapes_task_metadata_strings() {
        assert_eq!(
            escape_toml_string("quote \" slash \\ newline\n"),
            "quote \\\" slash \\\\ newline\\n"
        );
    }
}
