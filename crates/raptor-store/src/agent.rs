//! Agent-session store: `.raptor/agents/<agent-id>.toml`, one file per
//! session, written once with the session's own metadata encoding.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use raptor_agent::{AgentId, AgentSession};

use crate::atomic_io::atomic_create;
use crate::paths::{validate_base_dir, validate_child_path};
use crate::toml_codec::id_from_metadata_path;
use crate::{current_unix_timestamp, AGENTS_DIR, RAPTOR_STATE_DIR};

#[derive(Debug)]
pub enum AgentStoreError {
    AlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
    NotFound(String),
    Parse { path: PathBuf, message: String },
    UnsafeAgentId(String),
    UnsafeBasePath(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAgentSession {
    pub metadata_path: PathBuf,
    pub session: AgentSession,
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

pub fn persist_agent_session(
    repository_root: &Path,
    session: &AgentSession,
) -> Result<PathBuf, AgentStoreError> {
    validate_base_dir(repository_root).map_err(AgentStoreError::UnsafeBasePath)?;

    let agent_dir = agent_directory_path(repository_root)?;
    validate_child_path(repository_root, &agent_dir).map_err(AgentStoreError::UnsafeBasePath)?;

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
    validate_base_dir(repository_root).map_err(AgentStoreError::UnsafeBasePath)?;

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
        let Some(agent_id) = id_from_metadata_path(&path) else {
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
    validate_base_dir(repository_root).map_err(AgentStoreError::UnsafeBasePath)?;
    validate_agent_id(agent_id)?;

    let metadata_path = agent_directory_path(repository_root)?.join(format!("{agent_id}.toml"));
    validate_child_path(repository_root, &metadata_path)
        .map_err(AgentStoreError::UnsafeBasePath)?;

    Ok(metadata_path)
}

fn agent_directory_path(repository_root: &Path) -> Result<PathBuf, AgentStoreError> {
    validate_base_dir(repository_root).map_err(AgentStoreError::UnsafeBasePath)?;

    let agent_dir = repository_root.join(RAPTOR_STATE_DIR).join(AGENTS_DIR);
    validate_child_path(repository_root, &agent_dir).map_err(AgentStoreError::UnsafeBasePath)?;

    Ok(agent_dir)
}

fn validate_agent_id(agent_id: &str) -> Result<(), AgentStoreError> {
    AgentId::new(agent_id)
        .map(|_| ())
        .map_err(|_| AgentStoreError::UnsafeAgentId(agent_id.to_owned()))
}
