//! `raptor.toml` project configuration: creating the default file and parsing
//! the `version` and `[providers.<id>] enabled` entries.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use raptor_core::is_safe_hyphen_name;

use crate::atomic_io::atomic_create;
use crate::paths::validate_base_dir;
use crate::CONFIG_FILE_NAME;

const DEFAULT_CONFIG: &str =
    "# RaptorCode project configuration\nversion = 1\n\n[providers.deepseek]\nenabled = true\n";

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

pub fn init_project_config(base_dir: &Path) -> Result<InitConfigOutcome, InitConfigError> {
    validate_base_dir(base_dir).map_err(InitConfigError::UnsafeBasePath)?;

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
    validate_base_dir(base_dir).map_err(ProjectConfigError::UnsafeBasePath)?;

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

fn validate_provider_id(provider_id: &str) -> Result<(), ProjectConfigError> {
    if is_safe_hyphen_name(provider_id) {
        Ok(())
    } else {
        Err(ProjectConfigError::UnsafeProviderId(provider_id.to_owned()))
    }
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
