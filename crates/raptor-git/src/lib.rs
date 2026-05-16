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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergePreflight {
    pub changed_files: Vec<String>,
    pub status_lines: Vec<String>,
    pub target_branch: Option<String>,
    pub target_repository_path: PathBuf,
    pub worktree_branch: Option<String>,
    pub worktree_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeExecution {
    pub changed_files: Vec<String>,
    pub final_status_lines: Vec<String>,
    pub merge_stderr: String,
    pub merge_stdout: String,
    pub source_branch: String,
    pub target_branch: String,
    pub target_repository_path: PathBuf,
    pub worktree_path: PathBuf,
}

#[derive(Debug)]
pub enum GitError {
    BranchExists(String),
    CommandFailed {
        args: Vec<String>,
        stderr: String,
    },
    DirtyRepository(PathBuf),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    MergeConflict {
        path: PathBuf,
        files: Vec<String>,
        stderr: String,
    },
    MissingBranch {
        path: PathBuf,
        role: &'static str,
    },
    MissingWorktree(PathBuf),
    NotRepository(PathBuf),
    NoTaskChanges(PathBuf),
    SameBranch(String),
    UncommittedTaskChanges(PathBuf),
    UnresolvedConflicts {
        path: PathBuf,
        files: Vec<String>,
    },
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
    let status_lines = status_lines(&worktree_path)?;
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

pub fn preflight_task_merge(
    repository_root: &RepositoryRoot,
    task_id: &str,
    stored_worktree_path: &Path,
) -> Result<MergePreflight, GitError> {
    let worktree_path =
        validate_task_worktree_path(repository_root, task_id, stored_worktree_path)?;
    let target_conflicts = unresolved_conflict_files(repository_root.path())?;

    if !target_conflicts.is_empty() {
        return Err(GitError::UnresolvedConflicts {
            path: repository_root.path().to_path_buf(),
            files: target_conflicts,
        });
    }

    if !repository_clean_for_merge(repository_root.path())? {
        return Err(GitError::DirtyRepository(
            repository_root.path().to_path_buf(),
        ));
    }

    let worktree_conflicts = unresolved_conflict_files(&worktree_path)?;

    if !worktree_conflicts.is_empty() {
        return Err(GitError::UnresolvedConflicts {
            path: worktree_path,
            files: worktree_conflicts,
        });
    }

    let status_lines = status_lines(&worktree_path)?;
    let target_branch = git_output_line(repository_root.path(), &["branch", "--show-current"]);
    let worktree_branch = git_output_line(&worktree_path, &["branch", "--show-current"]);
    let mut changed_files = status_lines
        .iter()
        .filter_map(|line| changed_file_from_status_line(line))
        .collect::<Vec<_>>();

    if let (Some(target_branch), Some(worktree_branch)) =
        (target_branch.as_deref(), worktree_branch.as_deref())
    {
        validate_detected_branch_name(repository_root.path(), target_branch)?;
        validate_detected_branch_name(repository_root.path(), worktree_branch)?;

        if target_branch != worktree_branch {
            for file in
                branch_changed_files(repository_root.path(), target_branch, worktree_branch)?
            {
                push_unique_changed_file(&mut changed_files, file);
            }
        }
    }

    if changed_files.is_empty() {
        return Err(GitError::NoTaskChanges(worktree_path));
    }

    Ok(MergePreflight {
        changed_files,
        status_lines,
        target_branch,
        target_repository_path: repository_root.path().to_path_buf(),
        worktree_branch,
        worktree_path,
    })
}

pub fn execute_task_merge(
    repository_root: &RepositoryRoot,
    task_id: &str,
    stored_worktree_path: &Path,
) -> Result<MergeExecution, GitError> {
    let preflight = preflight_task_merge(repository_root, task_id, stored_worktree_path)?;
    let target_branch = preflight
        .target_branch
        .clone()
        .ok_or_else(|| GitError::MissingBranch {
            path: repository_root.path().to_path_buf(),
            role: "target branch",
        })?;
    let source_branch =
        preflight
            .worktree_branch
            .clone()
            .ok_or_else(|| GitError::MissingBranch {
                path: preflight.worktree_path.clone(),
                role: "source branch",
            })?;

    validate_detected_branch_name(repository_root.path(), &target_branch)?;
    validate_detected_branch_name(repository_root.path(), &source_branch)?;

    if source_branch == target_branch {
        return Err(GitError::SameBranch(source_branch));
    }

    if !preflight.status_lines.is_empty() {
        return Err(GitError::UncommittedTaskChanges(preflight.worktree_path));
    }

    let changed_files =
        branch_changed_files(repository_root.path(), &target_branch, &source_branch)?;

    if changed_files.is_empty() {
        return Err(GitError::NoTaskChanges(preflight.worktree_path));
    }

    let source_ref = branch_ref(&source_branch);
    let output = Command::new("git")
        .arg("merge")
        .arg("--no-edit")
        .arg(&source_ref)
        .current_dir(repository_root.path())
        .output()
        .map_err(|source| GitError::Io {
            path: repository_root.path().to_path_buf(),
            source,
        })?;

    let merge_stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let merge_stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

    if !output.status.success() {
        let conflicts = unresolved_conflict_files(repository_root.path())?;

        if !conflicts.is_empty() {
            return Err(GitError::MergeConflict {
                path: repository_root.path().to_path_buf(),
                files: conflicts,
                stderr: merge_command_detail(&merge_stdout, &merge_stderr),
            });
        }

        return Err(GitError::CommandFailed {
            args: vec!["merge".to_owned(), "--no-edit".to_owned(), source_ref],
            stderr: merge_command_detail(&merge_stdout, &merge_stderr),
        });
    }

    Ok(MergeExecution {
        changed_files,
        final_status_lines: status_lines(repository_root.path())?,
        merge_stderr,
        merge_stdout,
        source_branch,
        target_branch,
        target_repository_path: repository_root.path().to_path_buf(),
        worktree_path: preflight.worktree_path,
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
            Self::DirtyRepository(_) => {
                formatter.write_str(
                    "target repository has uncommitted changes; commit, stash, or discard changes before merge preflight",
                )
            }
            Self::Io { path, source } => {
                write!(formatter, "{}: {source}", path.display())
            }
            Self::MergeConflict { files, stderr, .. } => {
                if files.is_empty() {
                    write!(
                        formatter,
                        "merge conflicts detected; no automatic resolution attempted{}",
                        display_command_detail(stderr)
                    )
                } else {
                    write!(
                        formatter,
                        "merge conflicts detected; no automatic resolution attempted: {}{}",
                        files.join(", "),
                        display_command_detail(stderr)
                    )
                }
            }
            Self::MissingBranch { role, .. } => {
                write!(formatter, "{role} could not be detected; merge execution requires a named branch")
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
            Self::NoTaskChanges(_) => {
                formatter.write_str("task worktree has no changes to merge")
            }
            Self::SameBranch(branch) => {
                write!(
                    formatter,
                    "source branch and target branch are the same: {branch}"
                )
            }
            Self::UncommittedTaskChanges(_) => {
                formatter.write_str(
                    "task worktree has uncommitted changes; commit or discard them before merge execution",
                )
            }
            Self::UnresolvedConflicts { files, .. } => {
                if files.is_empty() {
                    formatter.write_str("unresolved conflicts detected")
                } else {
                    write!(formatter, "unresolved conflicts detected: {}", files.join(", "))
                }
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
            | Self::DirtyRepository(_)
            | Self::MergeConflict { .. }
            | Self::MissingBranch { .. }
            | Self::MissingWorktree(_)
            | Self::NotRepository(_)
            | Self::NoTaskChanges(_)
            | Self::SameBranch(_)
            | Self::UncommittedTaskChanges(_)
            | Self::UnresolvedConflicts { .. }
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

fn branch_changed_files(
    repository_root: &Path,
    target_branch: &str,
    source_branch: &str,
) -> Result<Vec<String>, GitError> {
    let range = format!(
        "{}...{}",
        branch_ref(target_branch),
        branch_ref(source_branch)
    );
    let diff = git_output_text(repository_root, &["diff", "--name-only", range.as_str()])?;

    Ok(diff
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn branch_ref(branch_name: &str) -> String {
    format!("refs/heads/{branch_name}")
}

fn validate_detected_branch_name(
    repository_root: &Path,
    branch_name: &str,
) -> Result<(), GitError> {
    if branch_name.is_empty() || branch_name.starts_with('-') || branch_name.contains('\0') {
        return Err(GitError::UnsafeName(branch_name.to_owned()));
    }

    let output = Command::new("git")
        .arg("check-ref-format")
        .arg("--branch")
        .arg(branch_name)
        .current_dir(repository_root)
        .output()
        .map_err(|source| GitError::Io {
            path: repository_root.to_path_buf(),
            source,
        })?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::UnsafeName(branch_name.to_owned()))
    }
}

fn status_lines(path: &Path) -> Result<Vec<String>, GitError> {
    let status = git_output_text(path, &["status", "--short"])?;

    Ok(status
        .lines()
        .map(|line| line.trim_end().to_owned())
        .filter(|line| !line.is_empty())
        .collect())
}

fn repository_clean_for_merge(repository_root: &Path) -> Result<bool, GitError> {
    let dirty_lines = status_lines(repository_root)?
        .into_iter()
        .filter(|line| {
            changed_file_from_status_line(line)
                .map(|path| !path_is_raptor_state(&path))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();

    Ok(dirty_lines.is_empty())
}

fn unresolved_conflict_files(path: &Path) -> Result<Vec<String>, GitError> {
    Ok(status_lines(path)?
        .into_iter()
        .filter(|line| status_line_is_unresolved_conflict(line))
        .filter_map(|line| changed_file_from_status_line(&line))
        .collect())
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

fn status_line_is_unresolved_conflict(line: &str) -> bool {
    let mut characters = line.chars();
    let left = characters.next();
    let right = characters.next();

    matches!(
        (left, right),
        (Some('D'), Some('D'))
            | (Some('A'), Some('U'))
            | (Some('U'), Some('D'))
            | (Some('U'), Some('A'))
            | (Some('D'), Some('U'))
            | (Some('A'), Some('A'))
            | (Some('U'), Some('U'))
    )
}

fn push_unique_changed_file(files: &mut Vec<String>, file: String) {
    if !files.iter().any(|existing| existing == &file) {
        files.push(file);
    }
}

fn merge_command_detail(stdout: &str, stderr: &str) -> String {
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no stdout or stderr output".to_owned(),
        (false, true) => stdout.to_owned(),
        (true, false) => stderr.to_owned(),
        (false, false) => format!("{stdout}\n{stderr}"),
    }
}

fn display_command_detail(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!(" ({detail})")
    }
}

fn path_is_raptor_state(path: &str) -> bool {
    path == RAPTOR_STATE_DIR || path.starts_with(".raptor/")
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
