//! Swarm metadata store: `.raptor/swarms/<swarm-id>.toml`, one file per swarm
//! holding the run's status plus an `[[agents]]` entry per specialist, with
//! every persisted value validated against the paths it is allowed to name.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use raptor_core::{is_safe_hyphen_name, SwarmStatus, TaskId};

use crate::atomic_io::{atomic_create, atomic_write};
use crate::paths::{validate_base_dir, validate_child_path};
use crate::toml_codec::{
    id_from_metadata_path, parse_quoted_value, push_toml_path, push_toml_string,
};
use crate::{current_unix_timestamp, RAPTOR_STATE_DIR, SWARMS_DIR};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmMetadata {
    pub agents: Vec<SwarmAgentMetadata>,
    pub created_at: Option<String>,
    pub description: String,
    pub id: String,
    pub model: String,
    pub planning_status: Option<SwarmStatus>,
    pub provider: String,
    pub status: SwarmStatus,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmAgentMetadata {
    pub agent_id: String,
    pub branch: String,
    pub implementation_status: Option<SwarmStatus>,
    pub model: Option<String>,
    pub plan_file_path: Option<PathBuf>,
    pub planning_status: Option<SwarmStatus>,
    pub proposal_file_path: Option<PathBuf>,
    pub provider: Option<String>,
    pub role: String,
    pub status: SwarmStatus,
    pub worktree_path: PathBuf,
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

pub fn persist_swarm_metadata(
    repository_root: &Path,
    metadata: &SwarmMetadata,
) -> Result<PathBuf, SwarmStoreError> {
    validate_base_dir(repository_root).map_err(SwarmStoreError::UnsafeBasePath)?;
    validate_swarm_metadata(repository_root, metadata)?;

    let swarm_dir = swarm_directory_path(repository_root)?;
    validate_child_path(repository_root, &swarm_dir).map_err(SwarmStoreError::UnsafeBasePath)?;

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
    validate_base_dir(repository_root).map_err(SwarmStoreError::UnsafeBasePath)?;
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
    validate_base_dir(repository_root).map_err(SwarmStoreError::UnsafeBasePath)?;

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
        let Some(swarm_id) = id_from_metadata_path(&path) else {
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

fn swarm_metadata_path_for_id(
    repository_root: &Path,
    swarm_id: &str,
) -> Result<PathBuf, SwarmStoreError> {
    validate_base_dir(repository_root).map_err(SwarmStoreError::UnsafeBasePath)?;
    validate_swarm_id(swarm_id)?;

    let metadata_path = swarm_directory_path(repository_root)?.join(format!("{swarm_id}.toml"));
    validate_child_path(repository_root, &metadata_path)
        .map_err(SwarmStoreError::UnsafeBasePath)?;

    Ok(metadata_path)
}

fn swarm_directory_path(repository_root: &Path) -> Result<PathBuf, SwarmStoreError> {
    validate_base_dir(repository_root).map_err(SwarmStoreError::UnsafeBasePath)?;

    let swarm_dir = repository_root.join(RAPTOR_STATE_DIR).join(SWARMS_DIR);
    validate_child_path(repository_root, &swarm_dir).map_err(SwarmStoreError::UnsafeBasePath)?;

    Ok(swarm_dir)
}

fn validate_swarm_id(swarm_id: &str) -> Result<(), SwarmStoreError> {
    if is_safe_hyphen_name(swarm_id) {
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

    for agent in &metadata.agents {
        validate_safe_hyphen_value("agent role", &agent.role)?;
        validate_safe_hyphen_value("agent id", &agent.agent_id)?;
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
    if is_safe_hyphen_name(value) {
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
        && branch.split('/').all(is_safe_hyphen_name);

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

fn render_swarm_metadata(metadata: &SwarmMetadata) -> String {
    let mut output = String::new();

    push_toml_string(&mut output, "id", Some(&metadata.id));
    push_toml_string(&mut output, "description", Some(&metadata.description));
    push_toml_string(&mut output, "status", Some(metadata.status.as_str()));
    push_toml_string(
        &mut output,
        "planning_status",
        metadata.planning_status.map(SwarmStatus::as_str),
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
        push_toml_string(&mut output, "status", Some(agent.status.as_str()));
        push_toml_string(
            &mut output,
            "implementation_status",
            agent.implementation_status.map(SwarmStatus::as_str),
        );
        push_toml_string(
            &mut output,
            "planning_status",
            agent.planning_status.map(SwarmStatus::as_str),
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

#[derive(Default)]
struct ParsedSwarmAgent {
    agent_id: Option<String>,
    branch: Option<String>,
    implementation_status: Option<SwarmStatus>,
    model: Option<String>,
    plan_file_path: Option<PathBuf>,
    planning_status: Option<SwarmStatus>,
    proposal_file_path: Option<PathBuf>,
    provider: Option<String>,
    role: Option<String>,
    status: Option<SwarmStatus>,
    worktree_path: Option<PathBuf>,
}

fn parse_status(
    metadata_path: &Path,
    line_number: usize,
    value: &str,
) -> Result<SwarmStatus, SwarmStoreError> {
    SwarmStatus::parse(value).ok_or_else(|| SwarmStoreError::Parse {
        path: metadata_path.to_path_buf(),
        message: format!("line {line_number} has an unknown swarm status: {value}"),
    })
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
        // Placeholder until the required `status` key is parsed below.
        status: SwarmStatus::PendingModelExecution,
        updated_at: None,
    };
    let mut status: Option<SwarmStatus> = None;
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
        let value = parse_quoted_value(line_number, value.trim()).map_err(|message| {
            SwarmStoreError::Parse {
                path: metadata_path.to_path_buf(),
                message,
            }
        })?;

        if let Some(agent) = current_agent.as_mut() {
            match key {
                "role" => agent.role = Some(value),
                "agent_id" => agent.agent_id = Some(value),
                "branch" => agent.branch = Some(value),
                "worktree_path" => agent.worktree_path = Some(PathBuf::from(value)),
                "status" => agent.status = Some(parse_status(metadata_path, line_number, &value)?),
                "implementation_status" => {
                    agent.implementation_status =
                        Some(parse_status(metadata_path, line_number, &value)?)
                }
                "planning_status" => {
                    agent.planning_status = Some(parse_status(metadata_path, line_number, &value)?)
                }
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
            "status" => status = Some(parse_status(metadata_path, line_number, &value)?),
            "planning_status" => {
                parsed.planning_status = Some(parse_status(metadata_path, line_number, &value)?)
            }
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

    parsed.status = status.ok_or_else(|| SwarmStoreError::Parse {
        path: metadata_path.to_path_buf(),
        message: "missing swarm status".to_owned(),
    })?;
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

fn required_swarm_agent_field<T>(
    metadata_path: &Path,
    line_number: usize,
    field: &'static str,
    value: Option<T>,
) -> Result<T, SwarmStoreError> {
    value.ok_or_else(|| SwarmStoreError::Parse {
        path: metadata_path.to_path_buf(),
        message: format!("line {line_number} is missing agent field {field}"),
    })
}
