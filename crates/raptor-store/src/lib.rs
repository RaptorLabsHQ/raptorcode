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
    NotFound(String),
    Parse { path: PathBuf, message: String },
    UnsafeBasePath(PathBuf),
    UnsafeTaskId(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTaskMetadata {
    pub branch: Option<String>,
    pub created_at: Option<String>,
    pub description: Option<String>,
    pub id: String,
    pub metadata_path: PathBuf,
    pub repository_root: Option<PathBuf>,
    pub status: Option<String>,
    pub worktree_path: Option<PathBuf>,
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
    task_metadata_path_for_id(repository_root, task_id.as_str())
}

pub fn list_task_metadata(
    repository_root: &Path,
) -> Result<Vec<StoredTaskMetadata>, TaskStoreError> {
    validate_task_base_dir(repository_root)?;

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
        let Some(task_id) = task_id_from_metadata_path(&path) else {
            continue;
        };

        if validate_task_id(&task_id).is_err() {
            continue;
        }

        tasks.push(read_task_metadata(repository_root, &task_id)?);
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

fn task_metadata_path_for_id(
    repository_root: &Path,
    task_id: &str,
) -> Result<PathBuf, TaskStoreError> {
    validate_task_base_dir(repository_root)?;
    validate_task_id(task_id)?;

    let metadata_path = task_directory_path(repository_root)?.join(format!("{task_id}.toml"));
    validate_child_path(repository_root, &metadata_path)?;

    Ok(metadata_path)
}

fn task_directory_path(repository_root: &Path) -> Result<PathBuf, TaskStoreError> {
    validate_task_base_dir(repository_root)?;

    let task_dir = repository_root.join(RAPTOR_STATE_DIR).join(TASKS_DIR);
    validate_child_path(repository_root, &task_dir)?;

    Ok(task_dir)
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

fn validate_task_id(task_id: &str) -> Result<(), TaskStoreError> {
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
        Err(TaskStoreError::UnsafeTaskId(task_id.to_owned()))
    }
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

fn task_id_from_metadata_path(path: &Path) -> Option<String> {
    if path.extension()?.to_str()? != "toml" {
        return None;
    }

    Some(path.file_stem()?.to_str()?.to_owned())
}

fn parse_task_metadata(
    metadata_path: &Path,
    fallback_task_id: &str,
    content: &str,
) -> Result<StoredTaskMetadata, TaskStoreError> {
    let mut parsed = StoredTaskMetadata {
        branch: None,
        created_at: None,
        description: None,
        id: fallback_task_id.to_owned(),
        metadata_path: metadata_path.to_path_buf(),
        repository_root: None,
        status: None,
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
        let value = parse_quoted_value(metadata_path, line_index + 1, value.trim())?;

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
            "status" => parsed.status = Some(value),
            "branch" => parsed.branch = Some(value),
            "created" | "created_at" => parsed.created_at = Some(value),
            "repository_root" => parsed.repository_root = Some(PathBuf::from(value)),
            "worktree_path" => parsed.worktree_path = Some(PathBuf::from(value)),
            _ => {}
        }
    }

    Ok(parsed)
}

fn parse_quoted_value(
    metadata_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<String, TaskStoreError> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(TaskStoreError::Parse {
            path: metadata_path.to_path_buf(),
            message: format!("line {line_number} value is not a quoted string"),
        });
    }

    unescape_toml_string(metadata_path, line_number, &value[1..value.len() - 1])
}

fn unescape_toml_string(
    metadata_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<String, TaskStoreError> {
    let mut unescaped = String::new();
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }

        let Some(escaped) = characters.next() else {
            return Err(TaskStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message: format!("line {line_number} has an incomplete escape sequence"),
            });
        };

        match escaped {
            '\\' => unescaped.push('\\'),
            '"' => unescaped.push('"'),
            'n' => unescaped.push('\n'),
            'r' => unescaped.push('\r'),
            't' => unescaped.push('\t'),
            _ => {
                return Err(TaskStoreError::Parse {
                    path: metadata_path.to_path_buf(),
                    message: format!("line {line_number} has an unsupported escape sequence"),
                });
            }
        }
    }

    Ok(unescaped)
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
    fn list_returns_empty_when_no_tasks_exist() {
        let test_dir = TestDir::create("list-empty");

        assert!(list_task_metadata(&test_dir.path).unwrap().is_empty());
    }

    #[test]
    fn list_reads_one_persisted_task() {
        let test_dir = TestDir::create("list-one");
        let task_id = TaskId::from_description("List one task").unwrap();
        let metadata = test_metadata(&test_dir.path, task_id.clone(), "List one task");
        persist_task_metadata(&test_dir.path, &metadata).unwrap();

        let tasks = list_task_metadata(&test_dir.path).unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task_id.as_str());
        assert_eq!(tasks[0].status.as_deref(), Some("planned"));
        assert_eq!(tasks[0].description.as_deref(), Some("List one task"));
    }

    #[test]
    fn show_reads_existing_task() {
        let test_dir = TestDir::create("show");
        let task_id = TaskId::from_description("Show task").unwrap();
        let metadata = test_metadata(&test_dir.path, task_id.clone(), "Show task");
        persist_task_metadata(&test_dir.path, &metadata).unwrap();

        let task = read_task_metadata(&test_dir.path, task_id.as_str()).unwrap();

        assert_eq!(task.id, task_id.as_str());
        assert_eq!(task.description.as_deref(), Some("Show task"));
    }

    #[test]
    fn show_missing_task_returns_not_found() {
        let test_dir = TestDir::create("missing");

        let error = read_task_metadata(&test_dir.path, "missing-task").unwrap_err();

        assert!(matches!(error, TaskStoreError::NotFound(_)));
    }

    #[test]
    fn rejects_unsafe_task_id() {
        let test_dir = TestDir::create("unsafe");

        let error = read_task_metadata(&test_dir.path, "Unsafe").unwrap_err();

        assert!(matches!(error, TaskStoreError::UnsafeTaskId(_)));
    }

    #[test]
    fn rejects_path_traversal_task_id() {
        let test_dir = TestDir::create("traversal");

        let error = read_task_metadata(&test_dir.path, "../secret").unwrap_err();

        assert!(matches!(error, TaskStoreError::UnsafeTaskId(_)));
    }

    #[test]
    fn task_reads_preserve_metadata_contents() {
        let test_dir = TestDir::create("readonly");
        let task_id = TaskId::from_description("Read only").unwrap();
        let metadata = test_metadata(&test_dir.path, task_id.clone(), "Read only");
        let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();
        let before = fs::read_to_string(&metadata_path).unwrap();
        let before_entries = fs::read_dir(test_dir.path.join(RAPTOR_STATE_DIR).join(TASKS_DIR))
            .unwrap()
            .count();

        list_task_metadata(&test_dir.path).unwrap();
        read_task_metadata(&test_dir.path, task_id.as_str()).unwrap();

        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before);
        assert_eq!(
            fs::read_dir(test_dir.path.join(RAPTOR_STATE_DIR).join(TASKS_DIR))
                .unwrap()
                .count(),
            before_entries
        );
    }

    #[test]
    fn escapes_task_metadata_strings() {
        assert_eq!(
            escape_toml_string("quote \" slash \\ newline\n"),
            "quote \\\" slash \\\\ newline\\n"
        );
    }

    fn test_metadata(repository_root: &Path, task_id: TaskId, description: &str) -> TaskMetadata {
        TaskMetadata::new(
            task_id.clone(),
            description,
            TaskStatus::Planned,
            WorktreeInfo {
                branch_name: format!("raptor-task-{}", task_id.as_str()),
                path: repository_root
                    .join(RAPTOR_STATE_DIR)
                    .join("worktrees")
                    .join(task_id.as_str()),
                repository_root: repository_root.to_path_buf(),
            },
        )
    }
}
