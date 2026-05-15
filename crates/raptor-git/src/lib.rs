use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use raptor_core::{TaskId, WorktreeInfo};

const RAPTOR_STATE_DIR: &str = ".raptor";
const WORKTREES_DIR: &str = "worktrees";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiagnostic {
    pub branch: Option<String>,
    pub current_dir: PathBuf,
    pub git_dir: Option<String>,
    pub inside_work_tree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryRoot {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreePlan {
    pub branch_name: String,
    pub path: PathBuf,
    pub repository_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeReview {
    pub changed_files: Vec<String>,
    pub status_lines: Vec<String>,
    pub worktree_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeDiff {
    pub staged: String,
    pub unstaged: String,
    pub worktree_path: PathBuf,
}

#[derive(Debug)]
pub enum GitError {
    BranchExists(String),
    CommandFailed {
        args: Vec<String>,
        stderr: String,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    MissingWorktree(PathBuf),
    NotRepository(PathBuf),
    UnsafeName(String),
    UnsafePath(PathBuf),
    WorktreeExists(PathBuf),
}

pub fn inspect_repository(current_dir: &Path) -> RepositoryDiagnostic {
    RepositoryDiagnostic {
        branch: git_output_line(current_dir, &["branch", "--show-current"]),
        current_dir: current_dir.to_path_buf(),
        git_dir: git_output_line(current_dir, &["rev-parse", "--git-dir"]),
        inside_work_tree: git_output_line(current_dir, &["rev-parse", "--is-inside-work-tree"])
            .is_some_and(|value| value == "true"),
    }
}

pub fn inspect_repository_root(current_dir: &Path) -> Result<RepositoryRoot, GitError> {
    let metadata = fs::metadata(current_dir).map_err(|source| GitError::Io {
        path: current_dir.to_path_buf(),
        source,
    })?;

    if !metadata.is_dir() {
        return Err(GitError::UnsafePath(current_dir.to_path_buf()));
    }

    let root = git_output_line(current_dir, &["rev-parse", "--show-toplevel"])
        .ok_or_else(|| GitError::NotRepository(current_dir.to_path_buf()))?;
    let root_path = PathBuf::from(root);
    let canonical_root = root_path.canonicalize().map_err(|source| GitError::Io {
        path: root_path.clone(),
        source,
    })?;

    validate_safe_existing_path(&canonical_root)?;

    Ok(RepositoryRoot {
        path: canonical_root,
    })
}

pub fn create_task_worktree(
    repository_root: &RepositoryRoot,
    task_id: &TaskId,
) -> Result<WorktreeInfo, GitError> {
    let plan = plan_task_worktree(repository_root, task_id)?;

    if branch_exists(repository_root.path(), &plan.branch_name)? {
        return Err(GitError::BranchExists(plan.branch_name));
    }

    let parent = plan
        .path
        .parent()
        .ok_or_else(|| GitError::UnsafePath(plan.path.clone()))?;

    fs::create_dir_all(parent).map_err(|source| GitError::Io {
        path: parent.to_path_buf(),
        source,
    })?;

    let output = Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(&plan.branch_name)
        .arg(&plan.path)
        .current_dir(repository_root.path())
        .output()
        .map_err(|source| GitError::Io {
            path: repository_root.path().to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(GitError::CommandFailed {
            args: vec![
                "worktree".to_owned(),
                "add".to_owned(),
                "-b".to_owned(),
                plan.branch_name,
                plan.path.display().to_string(),
            ],
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(WorktreeInfo {
        branch_name: plan.branch_name,
        path: plan.path,
        repository_root: plan.repository_root,
    })
}

pub fn plan_task_worktree(
    repository_root: &RepositoryRoot,
    task_id: &TaskId,
) -> Result<WorktreePlan, GitError> {
    validate_worktree_name(task_id.as_str())?;

    let branch_name = format!("raptor-task-{}", task_id.as_str());
    validate_branch_name(&branch_name)?;

    let path = repository_root
        .path()
        .join(RAPTOR_STATE_DIR)
        .join(WORKTREES_DIR)
        .join(task_id.as_str());

    validate_path_inside(repository_root.path(), &path)?;

    if path.exists() {
        return Err(GitError::WorktreeExists(path));
    }

    Ok(WorktreePlan {
        branch_name,
        path,
        repository_root: repository_root.path().to_path_buf(),
    })
}

pub fn validate_branch_name(name: &str) -> Result<(), GitError> {
    validate_safe_name(name)
}

pub fn validate_worktree_name(name: &str) -> Result<(), GitError> {
    validate_safe_name(name)
}

pub fn inspect_task_worktree(
    repository_root: &RepositoryRoot,
    task_id: &str,
    stored_worktree_path: &Path,
) -> Result<WorktreeReview, GitError> {
    let worktree_path =
        validate_task_worktree_path(repository_root, task_id, stored_worktree_path)?;
    let status = git_output_text(&worktree_path, &["status", "--short"])?;
    let status_lines = status
        .lines()
        .map(|line| line.trim_end().to_owned())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let changed_files = status_lines
        .iter()
        .filter_map(|line| changed_file_from_status_line(line))
        .collect::<Vec<_>>();

    Ok(WorktreeReview {
        changed_files,
        status_lines,
        worktree_path,
    })
}

pub fn diff_task_worktree(
    repository_root: &RepositoryRoot,
    task_id: &str,
    stored_worktree_path: &Path,
) -> Result<WorktreeDiff, GitError> {
    let worktree_path =
        validate_task_worktree_path(repository_root, task_id, stored_worktree_path)?;
    let unstaged = git_output_text(&worktree_path, &["diff"])?;
    let staged = git_output_text(&worktree_path, &["diff", "--cached"])?;

    Ok(WorktreeDiff {
        staged,
        unstaged,
        worktree_path,
    })
}

impl RepositoryRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Display for GitError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BranchExists(branch) => {
                write!(formatter, "git branch already exists: {branch}")
            }
            Self::CommandFailed { args, stderr } => {
                write!(
                    formatter,
                    "git {} failed: {}",
                    args.join(" "),
                    if stderr.is_empty() {
                        "no stderr output"
                    } else {
                        stderr
                    }
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::MissingWorktree(path) => {
                write!(formatter, "worktree not found: {}", path.display())
            }
            Self::NotRepository(path) => {
                write!(
                    formatter,
                    "{} is not inside a git repository",
                    path.display()
                )
            }
            Self::UnsafeName(name) => {
                write!(formatter, "{name} is not a safe git name")
            }
            Self::UnsafePath(path) => {
                write!(
                    formatter,
                    "{} is not a safe repository path",
                    path.display()
                )
            }
            Self::WorktreeExists(path) => {
                write!(
                    formatter,
                    "worktree path already exists: {}",
                    path.display()
                )
            }
        }
    }
}

impl Error for GitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::BranchExists(_)
            | Self::CommandFailed { .. }
            | Self::MissingWorktree(_)
            | Self::NotRepository(_)
            | Self::UnsafeName(_)
            | Self::UnsafePath(_)
            | Self::WorktreeExists(_) => None,
        }
    }
}

fn git_output_line(current_dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    first_non_empty_line(&output.stdout)
}

fn git_output_text(current_dir: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()
        .map_err(|source| GitError::Io {
            path: current_dir.to_path_buf(),
            source,
        })?;

    if !output.status.success() {
        return Err(GitError::CommandFailed {
            args: args.iter().map(|argument| (*argument).to_owned()).collect(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn branch_exists(repository_root: &Path, branch_name: &str) -> Result<bool, GitError> {
    let ref_name = format!("refs/heads/{branch_name}");
    let output = Command::new("git")
        .arg("show-ref")
        .arg("--verify")
        .arg("--quiet")
        .arg(ref_name)
        .current_dir(repository_root)
        .output()
        .map_err(|source| GitError::Io {
            path: repository_root.to_path_buf(),
            source,
        })?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(GitError::CommandFailed {
            args: vec![
                "show-ref".to_owned(),
                "--verify".to_owned(),
                "--quiet".to_owned(),
                format!("refs/heads/{branch_name}"),
            ],
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        }),
    }
}

fn validate_safe_existing_path(path: &Path) -> Result<(), GitError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitError::UnsafePath(path.to_path_buf()));
    }

    Ok(())
}

fn validate_path_inside(root: &Path, candidate: &Path) -> Result<(), GitError> {
    if candidate
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitError::UnsafePath(candidate.to_path_buf()));
    }

    if !candidate.starts_with(root) {
        return Err(GitError::UnsafePath(candidate.to_path_buf()));
    }

    Ok(())
}

fn validate_task_worktree_path(
    repository_root: &RepositoryRoot,
    task_id: &str,
    stored_worktree_path: &Path,
) -> Result<PathBuf, GitError> {
    validate_worktree_name(task_id)?;

    if stored_worktree_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(GitError::UnsafePath(stored_worktree_path.to_path_buf()));
    }

    let state_dir = repository_root.path().join(RAPTOR_STATE_DIR);
    let worktrees_dir = state_dir.join(WORKTREES_DIR);
    let expected_path = worktrees_dir.join(task_id);

    validate_path_inside(repository_root.path(), &expected_path)?;

    if stored_worktree_path != expected_path {
        return Err(GitError::UnsafePath(stored_worktree_path.to_path_buf()));
    }

    ensure_existing_directory(&state_dir, &expected_path)?;
    ensure_existing_directory(&worktrees_dir, &expected_path)?;
    ensure_existing_directory(&expected_path, &expected_path)?;

    Ok(expected_path)
}

fn ensure_existing_directory(path: &Path, missing_worktree_path: &Path) -> Result<(), GitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(GitError::UnsafePath(path.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(
            GitError::MissingWorktree(missing_worktree_path.to_path_buf()),
        ),
        Err(source) => Err(GitError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validate_safe_name(name: &str) -> Result<(), GitError> {
    let valid = !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("..")
        && name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        });

    if valid {
        Ok(())
    } else {
        Err(GitError::UnsafeName(name.to_owned()))
    }
}

fn changed_file_from_status_line(line: &str) -> Option<String> {
    if line.len() < 4 {
        return None;
    }

    Some(line.chars().skip(3).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use raptor_core::TaskId;
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
            let path = std::env::temp_dir()
                .join(format!("raptor-git-{name}-{}-{nonce}", std::process::id()));

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
    fn reads_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line(b"\n\nmain\nsecondary\n"),
            Some("main".to_owned())
        );
    }

    #[test]
    fn validates_safe_branch_and_worktree_names() {
        assert!(validate_branch_name("raptor-task-example-123").is_ok());
        assert!(validate_worktree_name("example-123").is_ok());
        assert!(validate_branch_name("../main").is_err());
        assert!(validate_worktree_name("bad/name").is_err());
    }

    #[test]
    fn plans_deterministic_task_worktree_path() {
        let test_dir = TestDir::create("plan");
        let root = RepositoryRoot {
            path: test_dir.path.clone(),
        };
        let task_id = TaskId::from_description("Add router tests").unwrap();

        let first = plan_task_worktree(&root, &task_id).unwrap();
        let second = plan_task_worktree(&root, &task_id).unwrap();

        assert_eq!(first, second);
        assert!(first.path.starts_with(&test_dir.path));
        assert_eq!(
            first.branch_name,
            format!("raptor-task-{}", task_id.as_str())
        );
    }

    #[test]
    fn refuses_existing_worktree_path() {
        let test_dir = TestDir::create("exists");
        let root = RepositoryRoot {
            path: test_dir.path.clone(),
        };
        let task_id = TaskId::from_description("Existing path").unwrap();
        let existing_path = test_dir
            .path
            .join(".raptor")
            .join("worktrees")
            .join(task_id.as_str());
        fs::create_dir_all(existing_path).unwrap();

        let error = plan_task_worktree(&root, &task_id).unwrap_err();

        assert!(matches!(error, GitError::WorktreeExists(_)));
    }

    #[test]
    fn rejects_task_worktree_path_outside_expected_directory() {
        let test_dir = TestDir::create("unsafe-worktree");
        let root = RepositoryRoot {
            path: test_dir.path.clone(),
        };
        let task_id = TaskId::from_description("Unsafe worktree").unwrap();
        let unsafe_path = test_dir.path.join("outside").join(task_id.as_str());

        let error = validate_task_worktree_path(&root, task_id.as_str(), &unsafe_path).unwrap_err();

        assert!(matches!(error, GitError::UnsafePath(_)));
    }
}
