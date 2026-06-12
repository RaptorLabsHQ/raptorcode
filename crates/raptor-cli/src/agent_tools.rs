use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use raptor_providers::ToolDefinition;

pub(crate) fn describe_tool_call(name: &str, args_json: &str) -> String {
    let args =
        serde_json::from_str::<serde_json::Value>(args_json).unwrap_or(serde_json::Value::Null);
    match name {
        "run_command" => describe_shell_command(args["command"].as_str().unwrap_or("")),
        "write_file" => "Writing project file".to_owned(),
        "read_file" => "Reading project file".to_owned(),
        "list_directory" => "Inspecting project directory".to_owned(),
        "create_directory" => "Creating project directory".to_owned(),
        "change_directory" => "Changing working directory".to_owned(),
        _ => format!("Running {name}"),
    }
}

fn describe_shell_command(cmd: &str) -> String {
    let cmd = cmd.trim();
    // Skip "cd /path && " prefixes to surface the real command
    let effective = cmd
        .rfind(" && ")
        .map(|i| cmd[i + 4..].trim())
        .unwrap_or(cmd);
    let parts: Vec<&str> = effective.split_whitespace().collect();
    let base = parts.first().copied().unwrap_or("");

    match base {
        "npm" => match parts.get(1).copied().unwrap_or("") {
            "install" | "i" => {
                let pkgs: Vec<&str> = parts[2..]
                    .iter()
                    .filter(|s| !s.starts_with('-'))
                    .copied()
                    .collect();
                if pkgs.is_empty() {
                    "Installing npm dependencies".to_owned()
                } else {
                    format!("Installing: {}", pkgs[..pkgs.len().min(3)].join(", "))
                }
            }
            "run" => format!("Running script: {}", parts.get(2).copied().unwrap_or("...")),
            "create" => format!(
                "Scaffolding: {}",
                parts.get(2).copied().unwrap_or("project")
            ),
            "build" => "Building project (npm)".to_owned(),
            "test" => "Running tests (npm)".to_owned(),
            "start" => "Starting dev server".to_owned(),
            sub => format!("npm {sub}"),
        },
        "npx" => format!("Running: {}", parts.get(1).copied().unwrap_or("npx")),
        "pnpm" | "yarn" | "bun" => {
            let sub = parts.get(1).copied().unwrap_or("");
            let tool = base;
            match sub {
                "install" | "i" | "add" => format!("Installing deps ({tool})"),
                "build" => format!("Building ({tool})"),
                "run" => format!(
                    "Running script ({tool}): {}",
                    parts.get(2).copied().unwrap_or("...")
                ),
                _ => format!("{tool} {sub}"),
            }
        }
        "cargo" => match parts.get(1).copied().unwrap_or("") {
            "build" => "Compiling Rust project".to_owned(),
            "test" => "Running Rust tests".to_owned(),
            "install" => format!(
                "Installing crate: {}",
                parts.get(2).copied().unwrap_or("...")
            ),
            "run" => "Running Rust binary".to_owned(),
            sub => format!("cargo {sub}"),
        },
        "git" => match parts.get(1).copied().unwrap_or("") {
            "clone" => "Cloning repository".to_owned(),
            "add" => "Staging changes".to_owned(),
            "commit" => "Committing changes".to_owned(),
            "push" => "Pushing to remote".to_owned(),
            "pull" => "Pulling latest changes".to_owned(),
            "status" => "Checking git status".to_owned(),
            "init" => "Initializing git repository".to_owned(),
            sub => format!("git {sub}"),
        },
        "mkdir" => format!("Creating directories: {}", parts[1..].join(" ")),
        "cp" => "Copying files".to_owned(),
        "mv" => "Moving files".to_owned(),
        "rm" => "Removing files".to_owned(),
        "touch" => format!("Creating file: {}", parts.get(1).copied().unwrap_or("...")),
        "cat" | "tee" if effective.contains('>') => "Writing file content".to_owned(),
        "cat" => format!("Reading: {}", parts.get(1).copied().unwrap_or("...")),
        "find" => "Searching filesystem".to_owned(),
        "grep" | "rg" => "Searching file contents".to_owned(),
        "ls" => format!("Listing: {}", parts.get(1).copied().unwrap_or(".")),
        "python" | "python3" => format!(
            "Running Python: {}",
            parts.get(1).copied().unwrap_or("script")
        ),
        "node" => format!(
            "Running Node: {}",
            parts.get(1).copied().unwrap_or("script")
        ),
        "pip" | "pip3" => match parts.get(1).copied().unwrap_or("") {
            "install" => format!(
                "Installing Python pkg: {}",
                parts.get(2).copied().unwrap_or("...")
            ),
            sub => format!("pip {sub}"),
        },
        "chmod" | "chown" => "Setting permissions".to_owned(),
        "echo" | "printf" if effective.contains('>') => "Writing file content".to_owned(),
        _ => {
            let preview: String = effective.chars().take(55).collect();
            if effective.len() > 55 {
                format!("{preview}...")
            } else {
                preview
            }
        }
    }
}

fn resolve_tool_path(working_dir: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    }
}

pub(crate) fn build_agent_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "write_file".to_owned(),
            description: "Write content to a file. Creates the file and any parent directories if needed. Absolute and relative paths both accepted.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file (absolute or relative to cwd)" },
                    "content": { "type": "string", "description": "File content to write" }
                },
                "required": ["path", "content"]
            }),
        },
        ToolDefinition {
            name: "read_file".to_owned(),
            description: "Read the contents of a file. Absolute and relative paths both accepted.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "list_directory".to_owned(),
            description: "List files and directories. Supports absolute paths, relative paths, and .. traversal.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to list (default: current working directory)" }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "create_directory".to_owned(),
            description: "Create a directory and any missing parent directories.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the directory to create" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "change_directory".to_owned(),
            description: "Change the current working directory. Affects all subsequent tool calls. Supports absolute paths and .. traversal.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory to change to (absolute or relative)" }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "run_command".to_owned(),
            description: "Execute a shell command and return its output. Use for git, npm, cargo, find, grep, or any other system operation.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to run" },
                    "working_dir": { "type": "string", "description": "Directory to run the command in (default: current working directory)" }
                },
                "required": ["command"]
            }),
        },
    ]
}

pub(crate) fn execute_tool_call(working_dir: &mut PathBuf, name: &str, arguments: &str) -> String {
    let args: serde_json::Value = match serde_json::from_str(arguments) {
        Ok(v) => v,
        Err(e) => return format!("error: bad tool arguments: {e}"),
    };

    match name {
        "write_file" => {
            let Some(path_str) = args["path"].as_str() else {
                return "error: missing path argument".to_owned();
            };
            let Some(content) = args["content"].as_str() else {
                return "error: missing content argument".to_owned();
            };
            let target = resolve_tool_path(working_dir, path_str);
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    return format!("error: failed to create directories: {e}");
                }
            }
            match std::fs::write(&target, content) {
                Ok(()) => format!("wrote {} bytes to {}", content.len(), target.display()),
                Err(e) => format!("error: {e}"),
            }
        }
        "read_file" => {
            let Some(path_str) = args["path"].as_str() else {
                return "error: missing path argument".to_owned();
            };
            let target = resolve_tool_path(working_dir, path_str);
            match std::fs::read_to_string(&target) {
                Ok(content) => content,
                Err(e) => format!("error: {e}"),
            }
        }
        "list_directory" => {
            let path_str = args["path"].as_str().unwrap_or(".");
            let target = resolve_tool_path(working_dir, path_str);
            match std::fs::read_dir(&target) {
                Ok(entries) => {
                    let mut names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            if e.path().is_dir() {
                                format!("{name}/")
                            } else {
                                name
                            }
                        })
                        .collect();
                    names.sort();
                    if names.is_empty() {
                        format!("(empty) — {}", target.display())
                    } else {
                        format!("{}\n---\n{}", target.display(), names.join("\n"))
                    }
                }
                Err(e) => format!("error: {e}"),
            }
        }
        "create_directory" => {
            let Some(path_str) = args["path"].as_str() else {
                return "error: missing path argument".to_owned();
            };
            let target = resolve_tool_path(working_dir, path_str);
            match std::fs::create_dir_all(&target) {
                Ok(()) => format!("created {}", target.display()),
                Err(e) => format!("error: {e}"),
            }
        }
        "change_directory" => {
            let Some(path_str) = args["path"].as_str() else {
                return "error: missing path argument".to_owned();
            };
            let target = resolve_tool_path(working_dir, path_str);
            match target.canonicalize() {
                Ok(canonical) if canonical.is_dir() => {
                    *working_dir = canonical;
                    format!("cwd: {}", working_dir.display())
                }
                Ok(_) => format!("error: not a directory: {path_str}"),
                Err(e) => format!("error: {e}"),
            }
        }
        "run_command" => {
            let Some(cmd) = args["command"].as_str() else {
                return "error: missing command argument".to_owned();
            };
            let cmd = cmd.to_owned();
            let cmd_dir = if let Some(dir_str) = args["working_dir"].as_str() {
                resolve_tool_path(working_dir, dir_str)
            } else {
                working_dir.clone()
            };
            let mut command = std::process::Command::new("sh");
            command
                .arg("-c")
                .arg(&cmd)
                .current_dir(&cmd_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt as _;
                command.process_group(0);
            }
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(e) => return format!("error: {e}"),
            };
            let started_at = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => match child.wait_with_output() {
                        Ok(out) => {
                            let stdout_str = String::from_utf8_lossy(&out.stdout);
                            let stderr_str = String::from_utf8_lossy(&out.stderr);
                            let mut result = String::new();
                            if !stdout_str.is_empty() {
                                result.push_str(stdout_str.trim_end());
                            }
                            if !stderr_str.is_empty() {
                                if !result.is_empty() {
                                    result.push('\n');
                                }
                                result.push_str(&format!("[stderr] {}", stderr_str.trim_end()));
                            }
                            if result.len() > 4000 {
                                result.truncate(4000);
                                result.push_str("\n...[truncated]");
                            }
                            if result.is_empty() {
                                if out.status.success() {
                                    return "ok (no output)".to_owned();
                                } else {
                                    return format!(
                                        "exited with code {}",
                                        out.status.code().unwrap_or(-1)
                                    );
                                }
                            } else {
                                return result;
                            }
                        }
                        Err(e) => return format!("error: {e}"),
                    },
                    Ok(None) => {
                        if started_at.elapsed() >= Duration::from_secs(120) {
                            #[cfg(unix)]
                            {
                                let process_group = format!("-{}", child.id());
                                let _ = std::process::Command::new("kill")
                                    .arg("-TERM")
                                    .arg(&process_group)
                                    .status();
                                thread::sleep(Duration::from_millis(500));
                                let _ = std::process::Command::new("kill")
                                    .arg("-KILL")
                                    .arg(&process_group)
                                    .status();
                            }
                            let _ = child.kill();
                            let _ = child.wait();
                            return "error: command exceeded the 120-second execution limit and was stopped".to_owned();
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => return format!("error: {e}"),
                }
            }
        }
        unknown => format!("error: unknown tool: {unknown}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
                "raptor-agent-tools-{name}-{}-{nonce}",
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
    fn agent_tools_expose_expected_tool_names() {
        let names: Vec<String> = build_agent_tools()
            .into_iter()
            .map(|tool| tool.name)
            .collect();

        assert_eq!(
            names,
            vec![
                "write_file",
                "read_file",
                "list_directory",
                "create_directory",
                "change_directory",
                "run_command",
            ]
        );
    }

    #[test]
    fn resolve_tool_path_joins_relative_and_keeps_absolute_paths() {
        let working_dir = Path::new("/work/project");

        assert_eq!(
            resolve_tool_path(working_dir, "src/main.rs"),
            PathBuf::from("/work/project/src/main.rs")
        );
        assert_eq!(
            resolve_tool_path(working_dir, "/etc/hosts"),
            PathBuf::from("/etc/hosts")
        );
    }

    #[test]
    fn describe_tool_call_surfaces_effective_shell_command() {
        assert_eq!(
            describe_tool_call("run_command", "{\"command\":\"cd /tmp && cargo test\"}"),
            "Running Rust tests"
        );
        assert_eq!(
            describe_tool_call("write_file", "{\"path\":\"a\",\"content\":\"b\"}"),
            "Writing project file"
        );
    }

    #[test]
    fn execute_tool_call_writes_and_reads_files_relative_to_working_dir() {
        let test_dir = TestDir::create("write-read");
        let mut working_dir = test_dir.path.clone();

        let write_result = execute_tool_call(
            &mut working_dir,
            "write_file",
            "{\"path\":\"nested/note.txt\",\"content\":\"raptor\"}",
        );
        assert!(write_result.starts_with("wrote 6 bytes"), "{write_result}");

        let read_result = execute_tool_call(
            &mut working_dir,
            "read_file",
            "{\"path\":\"nested/note.txt\"}",
        );
        assert_eq!(read_result, "raptor");
    }

    #[test]
    fn execute_tool_call_change_directory_updates_working_dir() {
        let test_dir = TestDir::create("chdir");
        fs::create_dir_all(test_dir.path.join("inner")).unwrap();
        let mut working_dir = test_dir.path.clone();

        let result =
            execute_tool_call(&mut working_dir, "change_directory", "{\"path\":\"inner\"}");

        assert!(result.starts_with("cwd: "), "{result}");
        assert_eq!(
            working_dir,
            test_dir.path.join("inner").canonicalize().unwrap()
        );
    }

    #[test]
    fn execute_tool_call_rejects_unknown_tools_and_bad_arguments() {
        let mut working_dir = std::env::temp_dir();

        assert_eq!(
            execute_tool_call(&mut working_dir, "delete_everything", "{}"),
            "error: unknown tool: delete_everything"
        );
        assert!(
            execute_tool_call(&mut working_dir, "write_file", "not json")
                .starts_with("error: bad tool arguments")
        );
        assert_eq!(
            execute_tool_call(&mut working_dir, "write_file", "{\"content\":\"x\"}"),
            "error: missing path argument"
        );
    }

    #[cfg(unix)]
    #[test]
    fn execute_tool_call_run_command_captures_output() {
        let test_dir = TestDir::create("run");
        let mut working_dir = test_dir.path.clone();

        let result = execute_tool_call(
            &mut working_dir,
            "run_command",
            "{\"command\":\"echo raptor-ok\"}",
        );

        assert_eq!(result, "raptor-ok");
    }
}
