//! Local project state under `.raptor/` — tasks, swarms, agent sessions, and
//! config — persisted as a small hand-rolled TOML subset with atomic writes.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use raptor_agent::{AgentId, AgentSession};
use raptor_core::{TaskId, TaskMetadata};

pub const AGENTS_DIR: &str = "agents";
pub const CONFIG_FILE_NAME: &str = "raptor.toml";
pub const CRATE_PURPOSE: &str = "Local state storage boundaries for RaptorCode.";
pub const RAPTOR_STATE_DIR: &str = ".raptor";
pub const SWARMS_DIR: &str = "swarms";
pub const TASKS_DIR: &str = "tasks";

const DEFAULT_CONFIG: &str =
    "# RaptorCode project configuration\nversion = 1\n\n[providers.deepseek]\nenabled = true\n";

/// Monotonic counter for unique temp-file names during atomic writes.
static ATOMIC_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

/// Atomically writes `contents` to `path`: content is written to a sibling temp
/// file, flushed to disk, then renamed over the target. A crash can leave the
/// temp file behind but never a torn target — a concurrent or post-crash reader
/// always sees either the old file or the fully-written new one. `fs::rename`
/// replaces an existing destination on both Unix and Windows, so this serves
/// both create-new (via [`atomic_create`]) and in-place update.
fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "metadata".to_owned());
    let nonce = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(".{base}.tmp.{}.{nonce}", std::process::id()));

    let write = || -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    };
    if let Err(error) = write() {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }
    Ok(())
}

/// Atomically creates `path`, failing with `AlreadyExists` if it is already
/// present. The check→write window is acceptable for content-addressed ids and,
/// unlike `create_new` + a separate write, never leaves a torn/empty target that
/// would permanently wedge the id after a crash.
fn atomic_create(path: &Path, contents: &str) -> io::Result<()> {
    if path.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "metadata file already exists",
        ));
    }
    atomic_write(path, contents)
}

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
pub enum ProjectConfigError {
    Io { path: PathBuf, source: io::Error },
    NotFound(PathBuf),
    Parse { path: PathBuf, message: String },
    UnsafeBasePath(PathBuf),
    UnsafeProviderId(String),
}

#[derive(Debug)]
pub enum AgentStoreError {
    AlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
    NotFound(String),
    Parse { path: PathBuf, message: String },
    UnsafeAgentId(String),
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

#[derive(Debug)]
pub enum SwarmStoreError {
    AlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
    NotFound(String),
    UnsafeBasePath(PathBuf),
    UnsafeSwarmId(String),
    UnsafeValue { field: &'static str, value: String },
    Parse { path: PathBuf, message: String },
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
    pub status: Option<String>,
    pub updated_at: Option<String>,
    pub worktree_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectConfig {
    pub path: PathBuf,
    pub providers: Vec<ProjectProviderConfig>,
    pub version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectProviderConfig {
    pub enabled: bool,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentSession {
    pub metadata_path: PathBuf,
    pub session: AgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmMetadata {
    pub agents: Vec<SwarmAgentMetadata>,
    pub created_at: Option<String>,
    pub description: String,
    pub id: String,
    pub model: String,
    pub planning_status: Option<String>,
    pub provider: String,
    pub status: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmAgentMetadata {
    pub agent_id: String,
    pub branch: String,
    pub implementation_status: Option<String>,
    pub model: Option<String>,
    pub plan_file_path: Option<PathBuf>,
    pub planning_status: Option<String>,
    pub proposal_file_path: Option<PathBuf>,
    pub provider: Option<String>,
    pub role: String,
    pub status: String,
    pub worktree_path: PathBuf,
}

impl TaskStatusTransition {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Abandoned => "abandoned",
            Self::MergeConflict => "merge_conflict",
            Self::Merged => "merged",
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

impl Display for ProjectConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::NotFound(path) => {
                write!(formatter, "RaptorCode config not found: {}", path.display())
            }
            Self::Parse { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
            Self::UnsafeProviderId(provider_id) => {
                write!(formatter, "provider id is not safe: {provider_id}")
            }
        }
    }
}

impl Error for ProjectConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::NotFound(_)
            | Self::Parse { .. }
            | Self::UnsafeBasePath(_)
            | Self::UnsafeProviderId(_) => None,
        }
    }
}

impl Display for AgentStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "agent session metadata already exists: {}",
                    path.display()
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::NotFound(agent_id) => {
                write!(formatter, "agent session metadata not found: {agent_id}")
            }
            Self::Parse { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::UnsafeAgentId(agent_id) => {
                write!(formatter, "agent id is not safe: {agent_id}")
            }
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
        }
    }
}

impl Error for AgentStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::AlreadyExists(_)
            | Self::NotFound(_)
            | Self::Parse { .. }
            | Self::UnsafeAgentId(_)
            | Self::UnsafeBasePath(_) => None,
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

impl Display for SwarmStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(path) => {
                write!(
                    formatter,
                    "swarm metadata already exists: {}",
                    path.display()
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::NotFound(swarm_id) => {
                write!(formatter, "swarm metadata not found: {swarm_id}")
            }
            Self::Parse { path, message } => {
                write!(formatter, "{}: {message}", path.display())
            }
            Self::UnsafeBasePath(path) => {
                write!(formatter, "{} is not a safe project path", path.display())
            }
            Self::UnsafeSwarmId(swarm_id) => {
                write!(formatter, "swarm id is not safe: {swarm_id}")
            }
            Self::UnsafeValue { field, value } => {
                write!(formatter, "swarm metadata {field} is not safe: {value}")
            }
        }
    }
}

impl Error for SwarmStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::AlreadyExists(_)
            | Self::NotFound(_)
            | Self::Parse { .. }
            | Self::UnsafeBasePath(_)
            | Self::UnsafeSwarmId(_)
            | Self::UnsafeValue { .. } => None,
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

    match atomic_create(&config_path, DEFAULT_CONFIG) {
        Ok(()) => Ok(InitConfigOutcome::Created(config_path)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Ok(InitConfigOutcome::Exists(config_path))
        }
        Err(source) => Err(InitConfigError::Io {
            path: config_path,
            source,
        }),
    }
}

pub fn read_project_config(base_dir: &Path) -> Result<ProjectConfig, ProjectConfigError> {
    validate_project_config_base_dir(base_dir)?;

    let config_path = base_dir.join(CONFIG_FILE_NAME);

    let file_metadata = match fs::symlink_metadata(&config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ProjectConfigError::NotFound(config_path));
        }
        Err(source) => {
            return Err(ProjectConfigError::Io {
                path: config_path,
                source,
            });
        }
    };

    if !file_metadata.file_type().is_file() {
        return Err(ProjectConfigError::UnsafeBasePath(config_path));
    }

    let content = fs::read_to_string(&config_path).map_err(|source| ProjectConfigError::Io {
        path: config_path.clone(),
        source,
    })?;

    parse_project_config(&config_path, &content)
}

pub fn persist_agent_session(
    repository_root: &Path,
    session: &AgentSession,
) -> Result<PathBuf, AgentStoreError> {
    validate_agent_base_dir(repository_root)?;

    let agent_dir = agent_directory_path(repository_root)?;
    validate_agent_child_path(repository_root, &agent_dir)?;

    fs::create_dir_all(&agent_dir).map_err(|source| AgentStoreError::Io {
        path: agent_dir.clone(),
        source,
    })?;

    let metadata_path = agent_metadata_path(repository_root, &session.id)?;

    let mut stored_session = session.clone();
    let timestamp = current_unix_timestamp();

    if stored_session.created_at.is_none() {
        stored_session.created_at = Some(timestamp.clone());
    }
    stored_session.updated_at = Some(timestamp);
    stored_session.audit_summary = Some(format!(
        "{} audit entries",
        stored_session.audit_entries.len()
    ));

    atomic_create(&metadata_path, &stored_session.to_metadata()).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            AgentStoreError::AlreadyExists(metadata_path.clone())
        } else {
            AgentStoreError::Io {
                path: metadata_path.clone(),
                source,
            }
        }
    })?;

    Ok(metadata_path)
}

pub fn list_agent_sessions(
    repository_root: &Path,
) -> Result<Vec<StoredAgentSession>, AgentStoreError> {
    validate_agent_base_dir(repository_root)?;

    let agent_dir = agent_directory_path(repository_root)?;

    match fs::symlink_metadata(&agent_dir) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(AgentStoreError::UnsafeBasePath(agent_dir)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(AgentStoreError::Io {
                path: agent_dir,
                source,
            });
        }
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(&agent_dir).map_err(|source| AgentStoreError::Io {
        path: agent_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| AgentStoreError::Io {
            path: agent_dir.clone(),
            source,
        })?;
        let path = entry.path();
        let Some(agent_id) = agent_id_from_metadata_path(&path) else {
            continue;
        };

        if AgentId::new(&agent_id).is_err() {
            continue;
        }

        // Skip an unreadable/torn file rather than failing the whole listing.
        if let Ok(session) = read_agent_session(repository_root, &agent_id) {
            sessions.push(session);
        }
    }

    sessions.sort_by(|left, right| left.session.id.as_str().cmp(right.session.id.as_str()));

    Ok(sessions)
}

pub fn read_agent_session(
    repository_root: &Path,
    agent_id: &str,
) -> Result<StoredAgentSession, AgentStoreError> {
    validate_agent_id(agent_id)?;

    let agent_dir = agent_directory_path(repository_root)?;
    let metadata_path = agent_metadata_path_for_id(repository_root, agent_id)?;

    let file_metadata = match fs::symlink_metadata(&metadata_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(AgentStoreError::NotFound(agent_id.to_owned()));
        }
        Err(source) => {
            return Err(AgentStoreError::Io {
                path: metadata_path,
                source,
            });
        }
    };

    if !file_metadata.file_type().is_file() {
        return Err(AgentStoreError::NotFound(agent_id.to_owned()));
    }

    let canonical_agent_dir = agent_dir
        .canonicalize()
        .map_err(|source| AgentStoreError::Io {
            path: agent_dir.clone(),
            source,
        })?;
    let canonical_metadata_path =
        metadata_path
            .canonicalize()
            .map_err(|source| AgentStoreError::Io {
                path: metadata_path.clone(),
                source,
            })?;

    if !canonical_metadata_path.starts_with(canonical_agent_dir) {
        return Err(AgentStoreError::UnsafeBasePath(metadata_path));
    }

    let content = fs::read_to_string(&metadata_path).map_err(|source| AgentStoreError::Io {
        path: metadata_path.clone(),
        source,
    })?;
    let session =
        AgentSession::from_metadata(&content).map_err(|error| AgentStoreError::Parse {
            path: metadata_path.clone(),
            message: error.to_string(),
        })?;

    if session.id.as_str() != agent_id {
        return Err(AgentStoreError::Parse {
            path: metadata_path.clone(),
            message: "metadata id does not match file name".to_owned(),
        });
    }

    Ok(StoredAgentSession {
        metadata_path,
        session,
    })
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

pub fn persist_swarm_metadata(
    repository_root: &Path,
    metadata: &SwarmMetadata,
) -> Result<PathBuf, SwarmStoreError> {
    validate_swarm_base_dir(repository_root)?;
    validate_swarm_metadata(repository_root, metadata)?;

    let swarm_dir = swarm_directory_path(repository_root)?;
    validate_swarm_child_path(repository_root, &swarm_dir)?;

    fs::create_dir_all(&swarm_dir).map_err(|source| SwarmStoreError::Io {
        path: swarm_dir.clone(),
        source,
    })?;

    let metadata_path = swarm_metadata_path_for_id(repository_root, &metadata.id)?;
    let mut stored_metadata = metadata.clone();
    let timestamp = current_unix_timestamp();

    if stored_metadata.created_at.is_none() {
        stored_metadata.created_at = Some(timestamp.clone());
    }
    stored_metadata.updated_at = Some(timestamp);

    atomic_create(&metadata_path, &render_swarm_metadata(&stored_metadata)).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            SwarmStoreError::AlreadyExists(metadata_path.clone())
        } else {
            SwarmStoreError::Io {
                path: metadata_path.clone(),
                source,
            }
        }
    })?;

    Ok(metadata_path)
}

pub fn update_swarm_metadata(
    repository_root: &Path,
    metadata: &SwarmMetadata,
) -> Result<PathBuf, SwarmStoreError> {
    validate_swarm_base_dir(repository_root)?;
    validate_swarm_metadata(repository_root, metadata)?;

    let metadata_path = swarm_metadata_path_for_id(repository_root, &metadata.id)?;
    let file_metadata =
        fs::symlink_metadata(&metadata_path).map_err(|source| SwarmStoreError::Io {
            path: metadata_path.clone(),
            source,
        })?;

    if !file_metadata.file_type().is_file() {
        return Err(SwarmStoreError::UnsafeBasePath(metadata_path));
    }

    let timestamp = current_unix_timestamp();
    let mut stored_metadata = metadata.clone();

    if stored_metadata.created_at.is_none() {
        stored_metadata.created_at = Some(timestamp.clone());
    }
    stored_metadata.updated_at = Some(timestamp);

    atomic_write(&metadata_path, &render_swarm_metadata(&stored_metadata)).map_err(|source| {
        SwarmStoreError::Io {
            path: metadata_path.clone(),
            source,
        }
    })?;

    Ok(metadata_path)
}

pub fn list_swarm_metadata(repository_root: &Path) -> Result<Vec<SwarmMetadata>, SwarmStoreError> {
    validate_swarm_base_dir(repository_root)?;

    let swarm_dir = swarm_directory_path(repository_root)?;

    match fs::symlink_metadata(&swarm_dir) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(SwarmStoreError::UnsafeBasePath(swarm_dir)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(SwarmStoreError::Io {
                path: swarm_dir,
                source,
            });
        }
    }

    let mut swarms = Vec::new();

    for entry in fs::read_dir(&swarm_dir).map_err(|source| SwarmStoreError::Io {
        path: swarm_dir.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| SwarmStoreError::Io {
            path: swarm_dir.clone(),
            source,
        })?;
        let path = entry.path();
        let Some(swarm_id) = swarm_id_from_metadata_path(&path) else {
            continue;
        };

        if validate_swarm_id(&swarm_id).is_err() {
            continue;
        }

        // Skip an unreadable/torn file rather than failing the whole listing.
        if let Ok(metadata) = read_swarm_metadata(repository_root, &swarm_id) {
            swarms.push(metadata);
        }
    }

    swarms.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(swarms)
}

pub fn read_swarm_metadata(
    repository_root: &Path,
    swarm_id: &str,
) -> Result<SwarmMetadata, SwarmStoreError> {
    let metadata_path = swarm_metadata_path_for_id(repository_root, swarm_id)?;
    let file_metadata = match fs::symlink_metadata(&metadata_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(SwarmStoreError::NotFound(swarm_id.to_owned()));
        }
        Err(source) => {
            return Err(SwarmStoreError::Io {
                path: metadata_path,
                source,
            });
        }
    };

    if !file_metadata.file_type().is_file() {
        return Err(SwarmStoreError::UnsafeBasePath(metadata_path));
    }

    let swarm_dir = swarm_directory_path(repository_root)?;
    let canonical_swarm_dir = swarm_dir
        .canonicalize()
        .map_err(|source| SwarmStoreError::Io {
            path: swarm_dir.clone(),
            source,
        })?;
    let canonical_metadata_path =
        metadata_path
            .canonicalize()
            .map_err(|source| SwarmStoreError::Io {
                path: metadata_path.clone(),
                source,
            })?;

    if !canonical_metadata_path.starts_with(canonical_swarm_dir) {
        return Err(SwarmStoreError::UnsafeBasePath(metadata_path));
    }

    let content = fs::read_to_string(&metadata_path).map_err(|source| SwarmStoreError::Io {
        path: metadata_path.clone(),
        source,
    })?;

    parse_swarm_metadata(repository_root, &metadata_path, swarm_id, &content)
}

pub fn swarm_metadata_exists(
    repository_root: &Path,
    task_id: &TaskId,
) -> Result<bool, SwarmStoreError> {
    Ok(swarm_metadata_path(repository_root, task_id)?.exists())
}

pub fn swarm_metadata_path(
    repository_root: &Path,
    task_id: &TaskId,
) -> Result<PathBuf, SwarmStoreError> {
    swarm_metadata_path_for_id(repository_root, task_id.as_str())
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

    task.status = Some(transition.as_str().to_owned());
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
    validate_task_base_dir(repository_root)?;
    validate_task_id(task_id)?;

    let metadata_path = task_directory_path(repository_root)?.join(format!("{task_id}.toml"));
    validate_child_path(repository_root, &metadata_path)?;

    Ok(metadata_path)
}

fn swarm_metadata_path_for_id(
    repository_root: &Path,
    swarm_id: &str,
) -> Result<PathBuf, SwarmStoreError> {
    validate_swarm_base_dir(repository_root)?;
    validate_swarm_id(swarm_id)?;

    let metadata_path = swarm_directory_path(repository_root)?.join(format!("{swarm_id}.toml"));
    validate_swarm_child_path(repository_root, &metadata_path)?;

    Ok(metadata_path)
}

fn agent_metadata_path(
    repository_root: &Path,
    agent_id: &AgentId,
) -> Result<PathBuf, AgentStoreError> {
    agent_metadata_path_for_id(repository_root, agent_id.as_str())
}

fn agent_metadata_path_for_id(
    repository_root: &Path,
    agent_id: &str,
) -> Result<PathBuf, AgentStoreError> {
    validate_agent_base_dir(repository_root)?;
    validate_agent_id(agent_id)?;

    let metadata_path = agent_directory_path(repository_root)?.join(format!("{agent_id}.toml"));
    validate_agent_child_path(repository_root, &metadata_path)?;

    Ok(metadata_path)
}

fn agent_directory_path(repository_root: &Path) -> Result<PathBuf, AgentStoreError> {
    validate_agent_base_dir(repository_root)?;

    let agent_dir = repository_root.join(RAPTOR_STATE_DIR).join(AGENTS_DIR);
    validate_agent_child_path(repository_root, &agent_dir)?;

    Ok(agent_dir)
}

fn task_directory_path(repository_root: &Path) -> Result<PathBuf, TaskStoreError> {
    validate_task_base_dir(repository_root)?;

    let task_dir = repository_root.join(RAPTOR_STATE_DIR).join(TASKS_DIR);
    validate_child_path(repository_root, &task_dir)?;

    Ok(task_dir)
}

fn swarm_directory_path(repository_root: &Path) -> Result<PathBuf, SwarmStoreError> {
    validate_swarm_base_dir(repository_root)?;

    let swarm_dir = repository_root.join(RAPTOR_STATE_DIR).join(SWARMS_DIR);
    validate_swarm_child_path(repository_root, &swarm_dir)?;

    Ok(swarm_dir)
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

fn validate_project_config_base_dir(base_dir: &Path) -> Result<(), ProjectConfigError> {
    if base_dir.as_os_str().is_empty()
        || base_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ProjectConfigError::UnsafeBasePath(base_dir.to_path_buf()));
    }

    Ok(())
}

fn validate_agent_base_dir(base_dir: &Path) -> Result<(), AgentStoreError> {
    if base_dir.as_os_str().is_empty()
        || base_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(AgentStoreError::UnsafeBasePath(base_dir.to_path_buf()));
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

fn validate_swarm_base_dir(base_dir: &Path) -> Result<(), SwarmStoreError> {
    if base_dir.as_os_str().is_empty()
        || base_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(SwarmStoreError::UnsafeBasePath(base_dir.to_path_buf()));
    }

    Ok(())
}

fn validate_agent_child_path(base_dir: &Path, child_path: &Path) -> Result<(), AgentStoreError> {
    if child_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
        || !child_path.starts_with(base_dir)
    {
        return Err(AgentStoreError::UnsafeBasePath(child_path.to_path_buf()));
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

fn validate_swarm_child_path(base_dir: &Path, child_path: &Path) -> Result<(), SwarmStoreError> {
    if child_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
        || !child_path.starts_with(base_dir)
    {
        return Err(SwarmStoreError::UnsafeBasePath(child_path.to_path_buf()));
    }

    Ok(())
}

fn validate_agent_id(agent_id: &str) -> Result<(), AgentStoreError> {
    AgentId::new(agent_id)
        .map(|_| ())
        .map_err(|_| AgentStoreError::UnsafeAgentId(agent_id.to_owned()))
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

fn validate_swarm_id(swarm_id: &str) -> Result<(), SwarmStoreError> {
    if safe_hyphen_name(swarm_id) {
        Ok(())
    } else {
        Err(SwarmStoreError::UnsafeSwarmId(swarm_id.to_owned()))
    }
}

fn validate_swarm_metadata(
    repository_root: &Path,
    metadata: &SwarmMetadata,
) -> Result<(), SwarmStoreError> {
    validate_swarm_id(&metadata.id)?;
    validate_safe_hyphen_value("provider", &metadata.provider)?;
    validate_safe_hyphen_value("model", &metadata.model)?;
    validate_safe_status("status", &metadata.status)?;
    if let Some(planning_status) = metadata.planning_status.as_deref() {
        validate_safe_status("planning_status", planning_status)?;
    }

    for agent in &metadata.agents {
        validate_safe_hyphen_value("agent role", &agent.role)?;
        validate_safe_hyphen_value("agent id", &agent.agent_id)?;
        validate_safe_status("agent status", &agent.status)?;
        if let Some(implementation_status) = agent.implementation_status.as_deref() {
            validate_safe_status("agent implementation_status", implementation_status)?;
        }
        if let Some(planning_status) = agent.planning_status.as_deref() {
            validate_safe_status("agent planning_status", planning_status)?;
        }
        if let Some(provider) = agent.provider.as_deref() {
            validate_safe_hyphen_value("agent provider", provider)?;
        }
        if let Some(model) = agent.model.as_deref() {
            validate_safe_hyphen_value("agent model", model)?;
        }
        validate_swarm_branch_value(&agent.branch)?;
        validate_swarm_worktree_path(
            repository_root,
            &metadata.id,
            &agent.role,
            &agent.worktree_path,
        )?;
        if let Some(plan_file_path) = agent.plan_file_path.as_ref() {
            validate_swarm_plan_file_path(&agent.worktree_path, plan_file_path)?;
        }
        if let Some(proposal_file_path) = agent.proposal_file_path.as_ref() {
            validate_swarm_proposal_file_path(&agent.worktree_path, proposal_file_path)?;
        }
    }

    Ok(())
}

fn validate_safe_hyphen_value(field: &'static str, value: &str) -> Result<(), SwarmStoreError> {
    if safe_hyphen_name(value) {
        Ok(())
    } else {
        Err(SwarmStoreError::UnsafeValue {
            field,
            value: value.to_owned(),
        })
    }
}

fn validate_safe_status(field: &'static str, value: &str) -> Result<(), SwarmStoreError> {
    let valid = !value.is_empty()
        && !value.starts_with('_')
        && !value.ends_with('_')
        && !value.contains("..")
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'
                || character == '_'
        });

    if valid {
        Ok(())
    } else {
        Err(SwarmStoreError::UnsafeValue {
            field,
            value: value.to_owned(),
        })
    }
}

fn validate_swarm_branch_value(branch: &str) -> Result<(), SwarmStoreError> {
    let valid = !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.starts_with('/')
        && !branch.ends_with('/')
        && !branch.contains('\0')
        && !branch.contains("..")
        && branch.split('/').all(safe_hyphen_name);

    if valid {
        Ok(())
    } else {
        Err(SwarmStoreError::UnsafeValue {
            field: "branch",
            value: branch.to_owned(),
        })
    }
}

fn validate_swarm_worktree_path(
    repository_root: &Path,
    swarm_id: &str,
    role: &str,
    worktree_path: &Path,
) -> Result<(), SwarmStoreError> {
    let expected_path = repository_root
        .join(RAPTOR_STATE_DIR)
        .join("worktrees")
        .join(swarm_id)
        .join(role);

    if worktree_path.as_os_str().is_empty()
        || worktree_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || worktree_path != expected_path
    {
        return Err(SwarmStoreError::UnsafeBasePath(worktree_path.to_path_buf()));
    }

    Ok(())
}

fn validate_swarm_plan_file_path(
    worktree_path: &Path,
    plan_file_path: &Path,
) -> Result<(), SwarmStoreError> {
    let expected_path = worktree_path.join("RAPTOR_PLAN.md");

    if plan_file_path.as_os_str().is_empty()
        || plan_file_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || plan_file_path != expected_path
    {
        return Err(SwarmStoreError::UnsafeBasePath(
            plan_file_path.to_path_buf(),
        ));
    }

    Ok(())
}

fn validate_swarm_proposal_file_path(
    worktree_path: &Path,
    proposal_file_path: &Path,
) -> Result<(), SwarmStoreError> {
    let expected_path = worktree_path.join("RAPTOR_IMPLEMENTATION_PROPOSAL.md");

    if proposal_file_path.as_os_str().is_empty()
        || proposal_file_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        || proposal_file_path != expected_path
    {
        return Err(SwarmStoreError::UnsafeBasePath(
            proposal_file_path.to_path_buf(),
        ));
    }

    Ok(())
}

fn safe_hyphen_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("..")
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn validate_provider_id(provider_id: &str) -> Result<(), ProjectConfigError> {
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
        Err(ProjectConfigError::UnsafeProviderId(provider_id.to_owned()))
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
        status: Some(metadata.status.as_str().to_owned()),
        updated_at: Some(timestamp),
        worktree_path: Some(metadata.worktree.path.clone()),
    };

    render_stored_task_metadata(&task)
}

fn render_swarm_metadata(metadata: &SwarmMetadata) -> String {
    let mut output = String::new();

    push_toml_string(&mut output, "id", Some(&metadata.id));
    push_toml_string(&mut output, "description", Some(&metadata.description));
    push_toml_string(&mut output, "status", Some(&metadata.status));
    push_toml_string(
        &mut output,
        "planning_status",
        metadata.planning_status.as_deref(),
    );
    push_toml_string(&mut output, "provider", Some(&metadata.provider));
    push_toml_string(&mut output, "model", Some(&metadata.model));
    push_toml_string(&mut output, "created_at", metadata.created_at.as_deref());
    push_toml_string(&mut output, "updated_at", metadata.updated_at.as_deref());

    for agent in &metadata.agents {
        output.push('\n');
        output.push_str("[[agents]]\n");
        push_toml_string(&mut output, "role", Some(&agent.role));
        push_toml_string(&mut output, "agent_id", Some(&agent.agent_id));
        push_toml_string(&mut output, "branch", Some(&agent.branch));
        push_toml_path(&mut output, "worktree_path", Some(&agent.worktree_path));
        push_toml_string(&mut output, "status", Some(&agent.status));
        push_toml_string(
            &mut output,
            "implementation_status",
            agent.implementation_status.as_deref(),
        );
        push_toml_string(
            &mut output,
            "planning_status",
            agent.planning_status.as_deref(),
        );
        push_toml_path(&mut output, "plan_file_path", agent.plan_file_path.as_ref());
        push_toml_path(
            &mut output,
            "proposal_file_path",
            agent.proposal_file_path.as_ref(),
        );
        push_toml_string(&mut output, "provider", agent.provider.as_deref());
        push_toml_string(&mut output, "model", agent.model.as_deref());
    }

    output
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

fn swarm_id_from_metadata_path(path: &Path) -> Option<String> {
    if path.extension()?.to_str()? != "toml" {
        return None;
    }

    Some(path.file_stem()?.to_str()?.to_owned())
}

fn agent_id_from_metadata_path(path: &Path) -> Option<String> {
    if path.extension()?.to_str()? != "toml" {
        return None;
    }

    Some(path.file_stem()?.to_str()?.to_owned())
}

fn parse_project_config(
    config_path: &Path,
    content: &str,
) -> Result<ProjectConfig, ProjectConfigError> {
    let mut parsed = ProjectConfig {
        path: config_path.to_path_buf(),
        providers: Vec::new(),
        version: None,
    };
    let mut current_provider_id = None;

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') || trimmed.ends_with(']') {
            if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
                return Err(ProjectConfigError::Parse {
                    path: config_path.to_path_buf(),
                    message: format!("line {} has an invalid section header", line_index + 1),
                });
            }

            let section = &trimmed[1..trimmed.len() - 1];
            if let Some(provider_id) = section.strip_prefix("providers.") {
                validate_provider_id(provider_id)?;
                ensure_provider_config(&mut parsed.providers, provider_id);
                current_provider_id = Some(provider_id.to_owned());
            } else {
                current_provider_id = None;
            }

            continue;
        }

        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| ProjectConfigError::Parse {
                path: config_path.to_path_buf(),
                message: format!("line {} is not a key/value pair", line_index + 1),
            })?;
        let key = key.trim();
        let value = value.trim();

        if let Some(provider_id) = current_provider_id.as_deref() {
            if key == "enabled" {
                let enabled = parse_config_bool(config_path, line_index + 1, value)?;
                provider_config_mut(&mut parsed.providers, provider_id).enabled = enabled;
            }

            continue;
        }

        if key == "version" {
            parsed.version = Some(
                value
                    .parse::<u32>()
                    .map_err(|_| ProjectConfigError::Parse {
                        path: config_path.to_path_buf(),
                        message: format!("line {} has invalid version", line_index + 1),
                    })?,
            );
        }
    }

    Ok(parsed)
}

fn ensure_provider_config<'a>(
    providers: &'a mut Vec<ProjectProviderConfig>,
    provider_id: &str,
) -> &'a mut ProjectProviderConfig {
    if providers
        .iter()
        .all(|provider| provider.id.as_str() != provider_id)
    {
        providers.push(ProjectProviderConfig {
            enabled: false,
            id: provider_id.to_owned(),
        });
    }

    provider_config_mut(providers, provider_id)
}

fn provider_config_mut<'a>(
    providers: &'a mut [ProjectProviderConfig],
    provider_id: &str,
) -> &'a mut ProjectProviderConfig {
    providers
        .iter_mut()
        .find(|provider| provider.id.as_str() == provider_id)
        .expect("provider config was inserted before mutation")
}

fn parse_config_bool(
    config_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<bool, ProjectConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ProjectConfigError::Parse {
            path: config_path.to_path_buf(),
            message: format!("line {line_number} has invalid boolean"),
        }),
    }
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

#[derive(Default)]
struct ParsedSwarmAgent {
    agent_id: Option<String>,
    branch: Option<String>,
    implementation_status: Option<String>,
    model: Option<String>,
    plan_file_path: Option<PathBuf>,
    planning_status: Option<String>,
    proposal_file_path: Option<PathBuf>,
    provider: Option<String>,
    role: Option<String>,
    status: Option<String>,
    worktree_path: Option<PathBuf>,
}

fn parse_swarm_metadata(
    repository_root: &Path,
    metadata_path: &Path,
    fallback_swarm_id: &str,
    content: &str,
) -> Result<SwarmMetadata, SwarmStoreError> {
    let mut parsed = SwarmMetadata {
        agents: Vec::new(),
        created_at: None,
        description: String::new(),
        id: fallback_swarm_id.to_owned(),
        model: String::new(),
        planning_status: None,
        provider: String::new(),
        status: String::new(),
        updated_at: None,
    };
    let mut current_agent: Option<ParsedSwarmAgent> = None;

    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "[[agents]]" {
            if let Some(agent) = current_agent.take() {
                parsed
                    .agents
                    .push(parsed_swarm_agent(metadata_path, line_number, agent)?);
            }
            current_agent = Some(ParsedSwarmAgent::default());
            continue;
        }

        if trimmed.starts_with('[') || trimmed.ends_with(']') {
            return Err(SwarmStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message: format!("line {line_number} has an invalid section header"),
            });
        }

        let (key, value) = trimmed
            .split_once('=')
            .ok_or_else(|| SwarmStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message: format!("line {line_number} is not a key/value pair"),
            })?;
        let key = key.trim();
        let value = parse_swarm_quoted_value(metadata_path, line_number, value.trim())?;

        if let Some(agent) = current_agent.as_mut() {
            match key {
                "role" => agent.role = Some(value),
                "agent_id" => agent.agent_id = Some(value),
                "branch" => agent.branch = Some(value),
                "worktree_path" => agent.worktree_path = Some(PathBuf::from(value)),
                "status" => agent.status = Some(value),
                "implementation_status" => agent.implementation_status = Some(value),
                "planning_status" => agent.planning_status = Some(value),
                "plan_file_path" => agent.plan_file_path = Some(PathBuf::from(value)),
                "proposal_file_path" => agent.proposal_file_path = Some(PathBuf::from(value)),
                "provider" => agent.provider = Some(value),
                "model" => agent.model = Some(value),
                _ => {}
            }
            continue;
        }

        match key {
            "id" => {
                validate_swarm_id(&value)?;

                if value != fallback_swarm_id {
                    return Err(SwarmStoreError::Parse {
                        path: metadata_path.to_path_buf(),
                        message: "metadata id does not match file name".to_owned(),
                    });
                }

                parsed.id = value;
            }
            "description" => parsed.description = value,
            "status" => parsed.status = value,
            "planning_status" => parsed.planning_status = Some(value),
            "provider" => parsed.provider = value,
            "model" => parsed.model = value,
            "created_at" => parsed.created_at = Some(value),
            "updated_at" => parsed.updated_at = Some(value),
            _ => {}
        }
    }

    if let Some(agent) = current_agent.take() {
        parsed.agents.push(parsed_swarm_agent(
            metadata_path,
            content.lines().count(),
            agent,
        )?);
    }

    validate_swarm_metadata(repository_root, &parsed)?;

    Ok(parsed)
}

fn parsed_swarm_agent(
    metadata_path: &Path,
    line_number: usize,
    agent: ParsedSwarmAgent,
) -> Result<SwarmAgentMetadata, SwarmStoreError> {
    Ok(SwarmAgentMetadata {
        agent_id: required_swarm_agent_field(
            metadata_path,
            line_number,
            "agent_id",
            agent.agent_id,
        )?,
        branch: required_swarm_agent_field(metadata_path, line_number, "branch", agent.branch)?,
        implementation_status: agent.implementation_status,
        model: agent.model,
        plan_file_path: agent.plan_file_path,
        planning_status: agent.planning_status,
        proposal_file_path: agent.proposal_file_path,
        provider: agent.provider,
        role: required_swarm_agent_field(metadata_path, line_number, "role", agent.role)?,
        status: required_swarm_agent_field(metadata_path, line_number, "status", agent.status)?,
        worktree_path: agent.worktree_path.ok_or_else(|| SwarmStoreError::Parse {
            path: metadata_path.to_path_buf(),
            message: format!("line {line_number} is missing agent field worktree_path"),
        })?,
    })
}

fn required_swarm_agent_field(
    metadata_path: &Path,
    line_number: usize,
    field: &'static str,
    value: Option<String>,
) -> Result<String, SwarmStoreError> {
    value.ok_or_else(|| SwarmStoreError::Parse {
        path: metadata_path.to_path_buf(),
        message: format!("line {line_number} is missing agent field {field}"),
    })
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
    push_toml_string(&mut output, "status", task.status.as_deref());
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

fn push_toml_string(output: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push_str(&format!("{key} = \"{}\"\n", escape_toml_string(value)));
    }
}

fn push_toml_path(output: &mut String, key: &str, value: Option<&PathBuf>) {
    if let Some(value) = value {
        push_toml_string(output, key, Some(&value.display().to_string()));
    }
}

fn current_unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
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

fn parse_swarm_quoted_value(
    metadata_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<String, SwarmStoreError> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(SwarmStoreError::Parse {
            path: metadata_path.to_path_buf(),
            message: format!("line {line_number} value is not a quoted string"),
        });
    }

    unescape_swarm_toml_string(metadata_path, line_number, &value[1..value.len() - 1])
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

fn unescape_swarm_toml_string(
    metadata_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<String, SwarmStoreError> {
    let mut unescaped = String::new();
    let mut characters = value.chars();

    while let Some(character) = characters.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }

        let Some(escaped) = characters.next() else {
            return Err(SwarmStoreError::Parse {
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
                return Err(SwarmStoreError::Parse {
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
    use raptor_agent::{AgentId, AgentRole, AgentSession};
    use raptor_core::{
        TaskMetadata, TaskStatus, WorktreeInfo, SWARM_PENDING_MODEL_EXECUTION_STATUS,
    };
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
        let content = fs::read_to_string(config_path).unwrap();

        assert!(content.contains("version = 1"));
        assert!(content.contains("[providers.deepseek]"));
        assert!(content.contains("enabled = true"));
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
    fn reads_provider_config_from_project_config() {
        let test_dir = TestDir::create("provider-config");
        fs::write(
            test_dir.path.join(CONFIG_FILE_NAME),
            "version = 1\n\n[providers.deepseek]\nenabled = true\n\n[providers.openai]\nenabled = false\n",
        )
        .unwrap();

        let config = read_project_config(&test_dir.path).unwrap();

        assert_eq!(config.version, Some(1));
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[0].id, "deepseek");
        assert!(config.providers[0].enabled);
        assert_eq!(config.providers[1].id, "openai");
        assert!(!config.providers[1].enabled);
    }

    #[test]
    fn provider_config_handles_empty_provider_sections() {
        let test_dir = TestDir::create("provider-config-empty");
        fs::write(test_dir.path.join(CONFIG_FILE_NAME), "version = 1\n").unwrap();

        let config = read_project_config(&test_dir.path).unwrap();

        assert!(config.providers.is_empty());
    }

    #[test]
    fn provider_config_rejects_unsafe_provider_ids() {
        let test_dir = TestDir::create("provider-config-unsafe");
        fs::write(
            test_dir.path.join(CONFIG_FILE_NAME),
            "[providers.DeepSeek]\nenabled = true\n",
        )
        .unwrap();

        let error = read_project_config(&test_dir.path).unwrap_err();

        assert!(matches!(error, ProjectConfigError::UnsafeProviderId(_)));
    }

    #[test]
    fn missing_project_config_is_explicit() {
        let test_dir = TestDir::create("provider-config-missing");

        let error = read_project_config(&test_dir.path).unwrap_err();

        assert!(matches!(error, ProjectConfigError::NotFound(_)));
    }

    #[test]
    fn persists_agent_session_without_overwrite() {
        let test_dir = TestDir::create("agent-persist");
        let session = AgentSession::new_read_only(
            AgentId::new("agent-worker-123").unwrap(),
            AgentRole::Worker,
        );

        let metadata_path = persist_agent_session(&test_dir.path, &session).unwrap();

        assert!(metadata_path.exists());
        assert!(matches!(
            persist_agent_session(&test_dir.path, &session).unwrap_err(),
            AgentStoreError::AlreadyExists(_)
        ));
    }

    #[test]
    fn list_returns_empty_when_no_agents_exist() {
        let test_dir = TestDir::create("agent-list-empty");

        assert!(list_agent_sessions(&test_dir.path).unwrap().is_empty());
    }

    #[test]
    fn list_reads_one_agent_session() {
        let test_dir = TestDir::create("agent-list-one");
        let mut session = AgentSession::new_read_only(
            AgentId::new("agent-reviewer-123").unwrap(),
            AgentRole::Reviewer,
        );
        session.set_provider_id("deepseek").unwrap();
        persist_agent_session(&test_dir.path, &session).unwrap();

        let sessions = list_agent_sessions(&test_dir.path).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session.id.as_str(), "agent-reviewer-123");
        assert_eq!(sessions[0].session.role, AgentRole::Reviewer);
        assert_eq!(sessions[0].session.provider_id.as_deref(), Some("deepseek"));
        assert!(sessions[0].session.created_at.is_some());
        assert!(sessions[0].session.updated_at.is_some());
    }

    #[test]
    fn show_reads_existing_agent_session() {
        let test_dir = TestDir::create("agent-show");
        let mut session =
            AgentSession::new_read_only(AgentId::new("agent-1").unwrap(), AgentRole::Architect);
        session.set_model_id("deepseek-v4-pro").unwrap();
        persist_agent_session(&test_dir.path, &session).unwrap();

        let stored = read_agent_session(&test_dir.path, "agent-1").unwrap();

        assert_eq!(stored.session.id.as_str(), "agent-1");
        assert_eq!(stored.session.role, AgentRole::Architect);
        assert_eq!(stored.session.model_id.as_deref(), Some("deepseek-v4-pro"));
    }

    #[test]
    fn show_missing_agent_returns_not_found() {
        let test_dir = TestDir::create("agent-missing");

        let error = read_agent_session(&test_dir.path, "missing-agent").unwrap_err();

        assert!(matches!(error, AgentStoreError::NotFound(_)));
    }

    #[test]
    fn rejects_unsafe_agent_id() {
        let test_dir = TestDir::create("agent-unsafe");

        let error = read_agent_session(&test_dir.path, "../agent").unwrap_err();

        assert!(matches!(error, AgentStoreError::UnsafeAgentId(_)));
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
    fn atomic_write_replaces_existing_content_without_leftovers() {
        let test_dir = TestDir::create("atomic-write");
        let path = test_dir.path.join("data.txt");

        atomic_write(&path, "first").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");

        atomic_write(&path, "second-and-longer").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second-and-longer");

        let leftover_temp = fs::read_dir(&test_dir.path)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().contains(".tmp."));
        assert!(!leftover_temp, "atomic_write left a temp file behind");
    }

    #[test]
    fn list_task_metadata_skips_torn_files() {
        let test_dir = TestDir::create("task-list-torn");
        let task_id = TaskId::from_description("List one healthy task").unwrap();
        let metadata = TaskMetadata::new(
            task_id.clone(),
            "List one healthy task",
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
        persist_task_metadata(&test_dir.path, &metadata).unwrap();

        // A torn file with a valid-looking id but unparseable content must not
        // break listing of the healthy task.
        let torn_path = test_dir
            .path
            .join(RAPTOR_STATE_DIR)
            .join(TASKS_DIR)
            .join("corrupt-task-deadbeef.toml");
        fs::write(&torn_path, "this line has no equals sign\n").unwrap();
        assert!(read_task_metadata(&test_dir.path, "corrupt-task-deadbeef").is_err());

        let listed = list_task_metadata(&test_dir.path).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id.as_str(), task_id.as_str());
    }

    #[test]
    fn persists_swarm_metadata_without_overwrite() {
        let test_dir = TestDir::create("swarm");
        let task_id = TaskId::from_description("Swarm metadata").unwrap();
        let metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm metadata");

        let metadata_path = persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
        let content = fs::read_to_string(&metadata_path).unwrap();

        assert!(metadata_path.exists());
        assert!(swarm_metadata_exists(&test_dir.path, &task_id).unwrap());
        assert!(content.contains(&format!("id = \"{}\"", task_id.as_str())));
        assert!(content.contains("provider = \"deepseek\""));
        assert!(content.contains("model = \"deepseek-v4-pro\""));
        assert!(content.contains("[[agents]]"));
        assert!(content.contains("role = \"frontend-agent\""));
        assert!(matches!(
            persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err(),
            SwarmStoreError::AlreadyExists(_)
        ));
    }

    #[test]
    fn list_reads_persisted_swarm_metadata() {
        let test_dir = TestDir::create("swarm-list");
        let task_id = TaskId::from_description("Swarm list").unwrap();
        let metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm list");

        persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
        fs::write(
            test_dir
                .path
                .join(".raptor")
                .join("swarms")
                .join("ignore.txt"),
            "ignore",
        )
        .unwrap();

        let swarms = list_swarm_metadata(&test_dir.path).unwrap();

        assert_eq!(swarms.len(), 1);
        assert_eq!(swarms[0].id, task_id.as_str());
        assert_eq!(swarms[0].agents.len(), 1);
    }

    #[test]
    fn updates_swarm_planning_metadata() {
        let test_dir = TestDir::create("swarm-update");
        let task_id = TaskId::from_description("Swarm update").unwrap();
        let mut metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm update");
        let metadata_path = persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
        let plan_file = metadata.agents[0].worktree_path.join("RAPTOR_PLAN.md");
        let proposal_file = metadata.agents[0]
            .worktree_path
            .join("RAPTOR_IMPLEMENTATION_PROPOSAL.md");

        metadata.status = "planning_completed".to_owned();
        metadata.planning_status = Some("planning_completed".to_owned());
        metadata.agents[0].status = "planning_completed".to_owned();
        metadata.agents[0].implementation_status = Some("proposal_generated".to_owned());
        metadata.agents[0].planning_status = Some("planning_completed".to_owned());
        metadata.agents[0].plan_file_path = Some(plan_file.clone());
        metadata.agents[0].proposal_file_path = Some(proposal_file.clone());
        metadata.agents[0].provider = Some("deepseek".to_owned());
        metadata.agents[0].model = Some("deepseek-v4-pro".to_owned());

        let updated_path = update_swarm_metadata(&test_dir.path, &metadata).unwrap();
        let content = fs::read_to_string(&updated_path).unwrap();

        assert_eq!(updated_path, metadata_path);
        assert!(content.contains("planning_status = \"planning_completed\""));
        assert!(content.contains(&format!("plan_file_path = \"{}\"", plan_file.display())));
        assert!(content.contains("implementation_status = \"proposal_generated\""));
        assert!(content.contains(&format!(
            "proposal_file_path = \"{}\"",
            proposal_file.display()
        )));
        assert!(content.contains("provider = \"deepseek\""));
        assert!(content.contains("model = \"deepseek-v4-pro\""));
    }

    #[test]
    fn reads_swarm_metadata_with_planning_and_proposal_fields() {
        let test_dir = TestDir::create("swarm-read");
        let task_id = TaskId::from_description("Read swarm").unwrap();
        let mut metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Read swarm");
        let plan_file = metadata.agents[0].worktree_path.join("RAPTOR_PLAN.md");
        let proposal_file = metadata.agents[0]
            .worktree_path
            .join("RAPTOR_IMPLEMENTATION_PROPOSAL.md");

        metadata.planning_status = Some("planning_completed".to_owned());
        metadata.agents[0].implementation_status = Some("proposal_generated".to_owned());
        metadata.agents[0].planning_status = Some("planning_completed".to_owned());
        metadata.agents[0].plan_file_path = Some(plan_file.clone());
        metadata.agents[0].proposal_file_path = Some(proposal_file.clone());
        metadata.agents[0].provider = Some("deepseek".to_owned());
        metadata.agents[0].model = Some("deepseek-v4-pro".to_owned());
        persist_swarm_metadata(&test_dir.path, &metadata).unwrap();

        let stored = read_swarm_metadata(&test_dir.path, task_id.as_str()).unwrap();

        assert_eq!(stored.id, task_id.as_str());
        assert_eq!(stored.description, "Read swarm");
        assert_eq!(
            stored.planning_status.as_deref(),
            Some("planning_completed")
        );
        assert_eq!(stored.agents.len(), 1);
        assert_eq!(
            stored.agents[0].implementation_status.as_deref(),
            Some("proposal_generated")
        );
        assert_eq!(stored.agents[0].plan_file_path.as_ref(), Some(&plan_file));
        assert_eq!(
            stored.agents[0].proposal_file_path.as_ref(),
            Some(&proposal_file)
        );
    }

    #[test]
    fn swarm_metadata_rejects_unsafe_values() {
        let test_dir = TestDir::create("swarm-unsafe");
        let task_id = TaskId::from_description("Unsafe swarm metadata").unwrap();
        let mut metadata =
            test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");

        metadata.agents[0].worktree_path = test_dir.path.join("..").join("outside");
        let path_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
        assert!(matches!(path_error, SwarmStoreError::UnsafeBasePath(_)));

        let mut metadata =
            test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
        metadata.agents[0].branch = "../main".to_owned();
        let branch_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
        assert!(matches!(branch_error, SwarmStoreError::UnsafeValue { .. }));

        let mut metadata =
            test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
        metadata.agents[0].plan_file_path = Some(test_dir.path.join("outside-plan.md"));
        let plan_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
        assert!(matches!(plan_error, SwarmStoreError::UnsafeBasePath(_)));

        let mut metadata =
            test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
        metadata.agents[0].proposal_file_path = Some(test_dir.path.join("outside-proposal.md"));
        let proposal_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
        assert!(matches!(proposal_error, SwarmStoreError::UnsafeBasePath(_)));
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
        assert!(tasks[0].created_at.is_some());
        assert!(tasks[0].updated_at.is_some());
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
    fn updates_task_status_with_transition_timestamp() {
        let test_dir = TestDir::create("status-update");
        let task_id = TaskId::from_description("Update status").unwrap();
        let metadata = test_metadata(&test_dir.path, task_id.clone(), "Update status");
        persist_task_metadata(&test_dir.path, &metadata).unwrap();

        let updated = update_task_status(
            &test_dir.path,
            task_id.as_str(),
            TaskStatusTransition::Merged,
        )
        .unwrap();

        assert_eq!(updated.status.as_deref(), Some("merged"));
        assert!(updated.updated_at.is_some());
        assert!(updated.merged_at.is_some());
        assert_eq!(updated.conflict_at, None);
        assert_eq!(updated.abandoned_at, None);
    }

    #[test]
    fn marks_task_abandoned_without_deleting_metadata() {
        let test_dir = TestDir::create("status-abandoned");
        let task_id = TaskId::from_description("Abandon status").unwrap();
        let metadata = test_metadata(&test_dir.path, task_id.clone(), "Abandon status");
        let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();

        let updated = update_task_status(
            &test_dir.path,
            task_id.as_str(),
            TaskStatusTransition::Abandoned,
        )
        .unwrap();

        assert_eq!(updated.status.as_deref(), Some("abandoned"));
        assert!(updated.abandoned_at.is_some());
        assert!(metadata_path.exists());
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

    fn test_swarm_metadata(
        repository_root: &Path,
        task_id: TaskId,
        description: &str,
    ) -> SwarmMetadata {
        let role = "frontend-agent";

        SwarmMetadata {
            agents: vec![SwarmAgentMetadata {
                agent_id: format!("{}-{role}", task_id.as_str()),
                branch: format!("raptor/{}/{role}", task_id.as_str()),
                implementation_status: None,
                model: None,
                plan_file_path: None,
                planning_status: None,
                proposal_file_path: None,
                provider: None,
                role: role.to_owned(),
                status: SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned(),
                worktree_path: repository_root
                    .join(RAPTOR_STATE_DIR)
                    .join("worktrees")
                    .join(task_id.as_str())
                    .join(role),
            }],
            created_at: None,
            description: description.to_owned(),
            id: task_id.as_str().to_owned(),
            model: "deepseek-v4-pro".to_owned(),
            planning_status: None,
            provider: "deepseek".to_owned(),
            status: SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned(),
            updated_at: None,
        }
    }
}
