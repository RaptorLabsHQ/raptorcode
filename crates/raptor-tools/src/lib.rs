use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDiagnostic {
    pub available: bool,
    pub detail: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCommand {
    pub args: Vec<String>,
    pub program: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCommandOutput {
    pub command: TestCommand,
    pub exit_code: Option<i32>,
    pub stderr: String,
    pub stdout: String,
    pub success: bool,
    pub worktree_path: PathBuf,
}

#[derive(Debug)]
pub enum TestCommandError {
    BlockedSubcommand {
        program: String,
        detail: String,
    },
    Empty,
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    UnsafeArgument(String),
    UnsafeProgram(String),
}

pub fn command_version(command: &str, args: &[&str]) -> ToolDiagnostic {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => ToolDiagnostic {
            available: true,
            detail: first_non_empty_line(&output.stdout)
                .or_else(|| first_non_empty_line(&output.stderr))
                .unwrap_or_else(|| "version output unavailable".to_owned()),
            name: command.to_owned(),
        },
        Ok(output) => ToolDiagnostic {
            available: false,
            detail: format!("command exited with {}", output.status),
            name: command.to_owned(),
        },
        Err(error) => ToolDiagnostic {
            available: false,
            detail: error.to_string(),
            name: command.to_owned(),
        },
    }
}

pub fn detect_test_command(worktree_path: &Path) -> Option<TestCommand> {
    if worktree_path.join("Cargo.toml").is_file() {
        return Some(TestCommand {
            args: vec!["test".to_owned()],
            program: "cargo".to_owned(),
            source: "detected Cargo.toml".to_owned(),
        });
    }

    let package_json_path = worktree_path.join("package.json");
    if package_json_path.is_file()
        && std::fs::read_to_string(package_json_path)
            .map(|content| package_json_has_test_script(&content))
            .unwrap_or(false)
    {
        return Some(TestCommand {
            args: vec!["test".to_owned()],
            program: "npm".to_owned(),
            source: "detected package.json scripts.test".to_owned(),
        });
    }

    let makefile_path = worktree_path.join("Makefile");
    if makefile_path.is_file()
        && std::fs::read_to_string(makefile_path)
            .map(|content| makefile_has_test_target(&content))
            .unwrap_or(false)
    {
        return Some(TestCommand {
            args: vec!["test".to_owned()],
            program: "make".to_owned(),
            source: "detected Makefile test target".to_owned(),
        });
    }

    None
}

pub fn explicit_test_command(parts: &[String]) -> Result<TestCommand, TestCommandError> {
    let Some((program, args)) = parts.split_first() else {
        return Err(TestCommandError::Empty);
    };

    validate_test_program(program)?;
    for argument in args {
        validate_test_argument(argument)?;
    }
    validate_test_subcommand(program, args)?;

    Ok(TestCommand {
        args: args.to_vec(),
        program: program.to_owned(),
        source: "explicit CLI command".to_owned(),
    })
}

pub fn run_test_command(
    worktree_path: &Path,
    command: &TestCommand,
) -> Result<TestCommandOutput, TestCommandError> {
    let output = Command::new(&command.program)
        .args(&command.args)
        .current_dir(worktree_path)
        .output()
        .map_err(|source| TestCommandError::Io {
            path: worktree_path.to_path_buf(),
            source,
        })?;

    Ok(TestCommandOutput {
        command: command.clone(),
        exit_code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        success: output.status.success(),
        worktree_path: worktree_path.to_path_buf(),
    })
}

impl TestCommand {
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Display for TestCommandError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockedSubcommand { program, detail } => {
                write!(
                    formatter,
                    "{program} is not allowed for raptor test: {detail}"
                )
            }
            Self::Empty => formatter.write_str("explicit test command is required after --"),
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::UnsafeArgument(argument) => {
                write!(formatter, "{argument} is not a safe test command argument")
            }
            Self::UnsafeProgram(program) => {
                write!(formatter, "{program} is not an allowed test command")
            }
        }
    }
}

impl Error for TestCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::BlockedSubcommand { .. }
            | Self::Empty
            | Self::UnsafeArgument(_)
            | Self::UnsafeProgram(_) => None,
        }
    }
}

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn package_json_has_test_script(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| value.get("scripts").cloned())
        .and_then(|scripts| scripts.get("test").cloned())
        .is_some_and(|script| script.is_string())
}

fn makefile_has_test_target(content: &str) -> bool {
    content
        .lines()
        .map(str::trim_start)
        .any(|line| line.starts_with("test:"))
}

fn validate_test_program(program: &str) -> Result<(), TestCommandError> {
    let allowed = matches!(
        program,
        "cargo"
            | "npm"
            | "pnpm"
            | "yarn"
            | "bun"
            | "make"
            | "go"
            | "python"
            | "python3"
            | "pytest"
            | "uv"
    );
    let safe = !program.is_empty()
        && !program.starts_with('-')
        && !program.contains('/')
        && !program.contains('\\')
        && !program.contains('\0')
        && program.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        });

    if allowed && safe {
        Ok(())
    } else {
        Err(TestCommandError::UnsafeProgram(program.to_owned()))
    }
}

fn validate_test_argument(argument: &str) -> Result<(), TestCommandError> {
    if argument.is_empty() || argument.contains('\0') || argument_contains_unsafe_path(argument) {
        Err(TestCommandError::UnsafeArgument(argument.to_owned()))
    } else {
        Ok(())
    }
}

fn argument_contains_unsafe_path(argument: &str) -> bool {
    argument.split('=').any(|segment| {
        segment.starts_with('/')
            || segment.starts_with('\\')
            || segment
                .split(['/', '\\'])
                .any(|component| component == "..")
    })
}

fn validate_test_subcommand(program: &str, args: &[String]) -> Result<(), TestCommandError> {
    let starts_with = |expected: &[&str]| {
        args.len() >= expected.len()
            && args
                .iter()
                .zip(expected.iter())
                .all(|(left, right)| left == right)
    };
    let contains_test_target = || args.iter().any(|argument| argument == "test");
    let allowed = match program {
        "cargo" => starts_with(&["test"]),
        "npm" | "pnpm" | "yarn" | "bun" => {
            starts_with(&["test"])
                || starts_with(&["run", "test"])
                || starts_with(&["run-script", "test"])
        }
        "make" => contains_test_target(),
        "go" => starts_with(&["test"]),
        "python" | "python3" => starts_with(&["-m", "pytest"]),
        "pytest" => true,
        "uv" => {
            starts_with(&["run", "pytest"])
                || starts_with(&["run", "python", "-m", "pytest"])
                || starts_with(&["run", "python3", "-m", "pytest"])
        }
        _ => false,
    };

    if allowed {
        Ok(())
    } else {
        Err(TestCommandError::BlockedSubcommand {
            program: program.to_owned(),
            detail:
                "only test-oriented subcommands are allowed; dependency installation is not allowed"
                    .to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line(b"\n\ncargo 1.0\n"),
            Some("cargo 1.0".to_owned())
        );
    }

    #[test]
    fn reports_missing_command_as_unavailable() {
        let diagnostic = command_version("raptor-code-missing-command", &["--version"]);

        assert!(!diagnostic.available);
        assert_eq!(diagnostic.name, "raptor-code-missing-command");
    }

    #[test]
    fn detects_common_test_commands() {
        let root =
            std::env::temp_dir().join(format!("raptor-tools-test-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
        assert_eq!(detect_test_command(&root).unwrap().display(), "cargo test");
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();

        std::fs::write(
            root.join("package.json"),
            "{\"scripts\":{\"test\":\"node test.js\"}}",
        )
        .unwrap();
        assert_eq!(detect_test_command(&root).unwrap().display(), "npm test");
        std::fs::remove_file(root.join("package.json")).unwrap();

        std::fs::write(root.join("Makefile"), "test:\n\ttrue\n").unwrap();
        assert_eq!(detect_test_command(&root).unwrap().display(), "make test");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn validates_explicit_test_commands() {
        let command =
            explicit_test_command(&["cargo".to_owned(), "test".to_owned(), "--quiet".to_owned()])
                .unwrap();

        assert_eq!(command.display(), "cargo test --quiet");
        assert!(explicit_test_command(&["npm".to_owned(), "install".to_owned()]).is_err());
        assert!(explicit_test_command(&[
            "bash".to_owned(),
            "-lc".to_owned(),
            "cargo test".to_owned()
        ])
        .is_err());
        assert!(explicit_test_command(&[
            "cargo".to_owned(),
            "test".to_owned(),
            "--manifest-path=../Cargo.toml".to_owned()
        ])
        .is_err());
        assert!(explicit_test_command(&[
            "cargo".to_owned(),
            "test".to_owned(),
            "--target-dir=/tmp/raptor".to_owned()
        ])
        .is_err());
        assert!(explicit_test_command(&[]).is_err());
    }
}
