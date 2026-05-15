use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

pub const CONFIG_FILE_NAME: &str = "raptor.toml";
pub const CRATE_PURPOSE: &str = "Local state storage boundaries for RaptorCode.";

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
