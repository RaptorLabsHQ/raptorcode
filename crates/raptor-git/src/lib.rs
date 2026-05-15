use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDiagnostic {
    pub branch: Option<String>,
    pub current_dir: PathBuf,
    pub git_dir: Option<String>,
    pub inside_work_tree: bool,
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

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line(b"\n\nmain\nsecondary\n"),
            Some("main".to_owned())
        );
    }
}
