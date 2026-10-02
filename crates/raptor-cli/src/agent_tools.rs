use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use raptor_providers::ToolDefinition;

/// Wall-clock ceiling for one `run_command` tool call.
const RUN_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);
/// Combined stdout/stderr returned to the model is capped at this many bytes.
const RUN_COMMAND_OUTPUT_CAP_BYTES: usize = 4000;
/// `read_file` content returned to the model is capped at this many bytes so a
/// large log or generated file cannot flood the context window in one call.
const READ_FILE_OUTPUT_CAP_BYTES: usize = 200_000;

/// Returns the host shell program and the flag that tells it to run a command
/// string: `sh -c` on Unix/macOS, `cmd /C` on Windows. This keeps the agent's
/// `run_command` tool functional across platforms instead of assuming `sh`.
fn platform_shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    }
}

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

/// Truncates `text` to at most `max_bytes`, backing up to the nearest UTF-8
/// character boundary. `String::truncate` panics on a non-boundary index, which
/// arbitrary command output (cargo/npm logs with multi-byte characters) can hit.
fn truncate_on_char_boundary(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

/// Reads at most the cap from a regular file. Devices, FIFOs, and directories
/// are refused up front: reading one of those whole could block forever or
/// grow without bound, which a cap applied after a full read cannot prevent.
fn read_file_capped(target: &Path) -> Result<String, String> {
    let metadata = std::fs::metadata(target).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!("not a regular file: {}", target.display()));
    }
    let total_bytes = metadata.len();
    let mut bytes = Vec::new();
    std::fs::File::open(target)
        .map_err(|error| error.to_string())?
        .take(READ_FILE_OUTPUT_CAP_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let truncated = bytes.len() > READ_FILE_OUTPUT_CAP_BYTES;
    let mut content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(error) => {
            // A cut through a multi-byte character at the cap is expected;
            // anything else is a binary file the model should not ingest.
            let valid_up_to = error.utf8_error().valid_up_to();
            let bytes = error.into_bytes();
            if !truncated || bytes.len() - valid_up_to > 3 {
                return Err(format!("not valid UTF-8 text: {}", target.display()));
            }
            String::from_utf8_lossy(&bytes[..valid_up_to]).into_owned()
        }
    };
    if truncated {
        truncate_on_char_boundary(&mut content, READ_FILE_OUTPUT_CAP_BYTES);
        content.push_str(&format!(
            "\n...[truncated: showing the first {READ_FILE_OUTPUT_CAP_BYTES} of {total_bytes} bytes]"
        ));
    }
    Ok(content)
}

/// Runs `cmd` through the platform shell inside `cmd_dir` and returns the tool
/// result the model sees. stdout and stderr are drained on background threads
/// from the moment the child starts: a child that writes more than the kernel
/// pipe buffer (64 KiB on Linux) otherwise blocks on `write()` and can never
/// exit, which the polling loop would misreport as a timeout. stdin is closed
/// so a command that prompts for input fails fast instead of hanging (or, in
/// the live swarm, fighting the dashboard for the terminal).
fn run_shell_command(cmd: &str, cmd_dir: &Path, timeout: Duration) -> String {
    let (shell_program, shell_flag) = platform_shell();
    let mut command = Command::new(shell_program);
    command
        .arg(shell_flag)
        .arg(cmd)
        .current_dir(cmd_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return format!("error: {error}"),
    };
    let stdout = drain_in_background(child.stdout.take());
    let stderr = drain_in_background(child.stderr.take());
    let started_at = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // A grandchild the shell backgrounded (a dev server, say) can
                // keep the pipes open after the shell itself has exited, so
                // the drains are collected with a deadline rather than joined.
                let deadline = Instant::now() + OUTPUT_DRAIN_GRACE;
                let (stdout, stdout_closed) = collect_drained(stdout, deadline);
                let (stderr, stderr_closed) = collect_drained(stderr, deadline);
                let mut result = render_command_output(status, &stdout, &stderr);
                if !(stdout_closed && stderr_closed) {
                    result.push_str(
                        "\n[note] a background process started by this command is still running and holding its output stream open",
                    );
                }
                return result;
            }
            Ok(None) if started_at.elapsed() >= timeout => {
                terminate_process_tree(&mut child);
                // The drain threads end on their own once every process holding
                // the pipes is gone; they are deliberately not joined so a
                // straggler can never hang the agent loop.
                return format!(
                    "error: command exceeded the {}-second execution limit and was stopped",
                    timeout.as_secs()
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return format!("error: {error}"),
        }
    }
}

/// How long to wait for the output pipes to close after the shell exits before
/// returning what was captured so far.
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// A pipe being read on a background thread into a shared buffer, so the
/// bytes captured so far are available even if the pipe never closes.
struct Drain {
    buffer: Arc<Mutex<Vec<u8>>>,
    handle: JoinHandle<()>,
}

fn drain_in_background<R: Read + Send + 'static>(pipe: Option<R>) -> Option<Drain> {
    let mut pipe = pipe?;
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&buffer);
    let handle = thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut sink) = sink.lock() {
                        sink.extend_from_slice(&chunk[..read]);
                    }
                }
            }
        }
    });
    Some(Drain { buffer, handle })
}

/// Waits until the drain's pipe closes or `deadline` passes, then returns the
/// captured bytes and whether the pipe had closed.
fn collect_drained(drain: Option<Drain>, deadline: Instant) -> (Vec<u8>, bool) {
    let Some(drain) = drain else {
        return (Vec::new(), true);
    };
    while !drain.handle.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let closed = drain.handle.is_finished();
    let bytes = drain
        .buffer
        .lock()
        .map(|buffer| buffer.clone())
        .unwrap_or_default();
    (bytes, closed)
}

fn render_command_output(status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(stdout.trim_end());
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&format!("[stderr] {}", stderr.trim_end()));
    }
    if result.len() > RUN_COMMAND_OUTPUT_CAP_BYTES {
        truncate_on_char_boundary(&mut result, RUN_COMMAND_OUTPUT_CAP_BYTES);
        result.push_str("\n...[truncated]");
    }
    if result.is_empty() {
        if status.success() {
            "ok (no output)".to_owned()
        } else {
            format!("exited with code {}", status.code().unwrap_or(-1))
        }
    } else {
        result
    }
}

/// Stops the shell and everything it spawned: the Unix process group (the
/// child was started as its own group leader) or the Windows process tree
/// (`cmd /C` children such as a dev server survive killing cmd.exe alone).
fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child.id());
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(&process_group)
            .status();
        thread::sleep(Duration::from_millis(500));
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(&process_group)
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID"])
            .arg(child.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub(crate) fn resolve_tool_path(working_dir: &Path, path_str: &str) -> PathBuf {
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
            match read_file_capped(&target) {
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
            let cmd_dir = if let Some(dir_str) = args["working_dir"].as_str() {
                resolve_tool_path(working_dir, dir_str)
            } else {
                working_dir.clone()
            };
            run_shell_command(cmd, &cmd_dir, RUN_COMMAND_TIMEOUT)
        }
        unknown => format!("error: unknown tool: {unknown}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn truncate_on_char_boundary_never_splits_a_codepoint() {
        // A 2-byte 'é' straddling the limit must not panic; truncation backs up
        // to the boundary before it.
        let mut text = format!("{}é tail", "a".repeat(9));
        truncate_on_char_boundary(&mut text, 10);
        assert_eq!(text, "a".repeat(9));

        let mut ascii = "abcdef".to_owned();
        truncate_on_char_boundary(&mut ascii, 3);
        assert_eq!(ascii, "abc");

        let mut short = "hi".to_owned();
        truncate_on_char_boundary(&mut short, 10);
        assert_eq!(short, "hi");
    }

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
    fn platform_shell_matches_the_host_os() {
        let (program, flag) = platform_shell();
        if cfg!(windows) {
            assert_eq!((program, flag), ("cmd", "/C"));
        } else {
            assert_eq!((program, flag), ("sh", "-c"));
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

    #[test]
    fn read_file_output_is_capped_with_a_truncation_marker() {
        let test_dir = TestDir::create("read-cap");
        let mut working_dir = test_dir.path.clone();
        fs::write(
            test_dir.path.join("big.log"),
            "x".repeat(READ_FILE_OUTPUT_CAP_BYTES + 5_000),
        )
        .unwrap();

        let result = execute_tool_call(&mut working_dir, "read_file", "{\"path\":\"big.log\"}");

        assert!(
            result.ends_with("...[truncated: showing the first 200000 of 205000 bytes]"),
            "{}",
            &result[result.len().saturating_sub(80)..]
        );
        assert!(result.len() < READ_FILE_OUTPUT_CAP_BYTES + 200);
    }

    #[test]
    fn read_file_refuses_anything_but_a_regular_file() {
        let test_dir = TestDir::create("read-dir");
        let mut working_dir = test_dir.path.clone();
        fs::create_dir(test_dir.path.join("src")).unwrap();

        let result = execute_tool_call(&mut working_dir, "read_file", "{\"path\":\"src\"}");

        assert!(
            result.starts_with("error: not a regular file: "),
            "{result}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn read_file_refuses_device_files_instead_of_reading_them_forever() {
        let mut working_dir = std::env::temp_dir();

        // `/dev/zero` never reaches EOF; reading it whole would never return.
        let result = execute_tool_call(&mut working_dir, "read_file", "{\"path\":\"/dev/zero\"}");

        assert!(
            result.starts_with("error: not a regular file: "),
            "{result}"
        );
    }

    #[test]
    fn read_file_rejects_binary_content() {
        let test_dir = TestDir::create("read-binary");
        let mut working_dir = test_dir.path.clone();
        fs::write(test_dir.path.join("blob.bin"), [0xff, 0xfe, 0x00, b'A']).unwrap();

        let result = execute_tool_call(&mut working_dir, "read_file", "{\"path\":\"blob.bin\"}");

        assert!(
            result.starts_with("error: not valid UTF-8 text: "),
            "{result}"
        );
    }

    #[test]
    fn read_file_cap_cuts_cleanly_through_a_multibyte_character() {
        let test_dir = TestDir::create("read-multibyte");
        let mut working_dir = test_dir.path.clone();
        // Two bytes per character, so the cap lands between the bytes of one.
        fs::write(
            test_dir.path.join("accents.txt"),
            "\u{e9}".repeat(READ_FILE_OUTPUT_CAP_BYTES / 2 + 1),
        )
        .unwrap();

        let result = execute_tool_call(&mut working_dir, "read_file", "{\"path\":\"accents.txt\"}");

        assert!(
            result.ends_with("...[truncated: showing the first 200000 of 200002 bytes]"),
            "{}",
            &result[result.len().saturating_sub(80)..]
        );
        assert!(
            !result.contains('\u{fffd}'),
            "a cut through a character must not leave a replacement character"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_shell_command_returns_when_a_background_process_keeps_the_pipe_open() {
        let test_dir = TestDir::create("background");

        // The shell exits at once; the backgrounded sleep inherits stdout and
        // holds the pipe open well past the drain grace period.
        let started = Instant::now();
        let result = run_shell_command(
            "sleep 5 & echo started",
            &test_dir.path,
            Duration::from_secs(30),
        );

        assert!(started.elapsed() < Duration::from_secs(5), "{result}");
        assert!(result.starts_with("started"), "{result}");
        assert!(
            result.contains("[note] a background process started by this command is still running"),
            "{result}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_shell_command_drains_output_larger_than_the_pipe_buffer() {
        let test_dir = TestDir::create("big-output");

        // 300 KiB is far past the 64 KiB kernel pipe buffer. Before stdout was
        // drained concurrently, the child blocked on write() forever and the
        // call surfaced as a bogus execution-limit error.
        let result = run_shell_command(
            "head -c 300000 /dev/zero | tr '\\0' a",
            &test_dir.path,
            Duration::from_secs(10),
        );

        assert!(!result.starts_with("error:"), "{result}");
        assert!(result.ends_with("...[truncated]"), "{result}");
    }

    #[cfg(unix)]
    #[test]
    fn run_shell_command_reports_the_execution_limit_on_timeout() {
        let test_dir = TestDir::create("timeout");

        let result = run_shell_command("sleep 5", &test_dir.path, Duration::from_secs(1));

        assert_eq!(
            result,
            "error: command exceeded the 1-second execution limit and was stopped"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_shell_command_closes_stdin_so_prompting_commands_fail_fast() {
        let test_dir = TestDir::create("stdin");

        // `cat` with an open terminal stdin would wait for input until the
        // execution limit; with stdin closed it sees EOF and exits at once.
        let result = run_shell_command("cat", &test_dir.path, Duration::from_secs(5));

        assert_eq!(result, "ok (no output)");
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
