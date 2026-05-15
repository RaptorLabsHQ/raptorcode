use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::Path;

use raptor_core::{project_description, project_name, version, TaskId, TaskMetadata, TaskStatus};
use raptor_git::{
    create_task_worktree, inspect_repository, inspect_repository_root, plan_task_worktree,
};
use raptor_store::{
    init_project_config, list_task_metadata, persist_task_metadata, read_task_metadata,
    task_metadata_exists, InitConfigOutcome, StoredTaskMetadata,
};
use raptor_tools::command_version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Chat,
    Diff,
    Doctor,
    Help,
    Init,
    Merge,
    Review,
    Swarm,
    Task(TaskCommand),
    Test,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCommand {
    Create { description: String },
    List,
    Show { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliError {}

pub fn parse_args<I, S>(args: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();

    match args.as_slice() {
        [] => Ok(CliCommand::Help),
        [arg] if arg == "--help" || arg == "-h" => Ok(CliCommand::Help),
        [arg] if arg == "--version" || arg == "-V" => Ok(CliCommand::Version),
        [arg] if arg == "init" => Ok(CliCommand::Init),
        [arg] if arg == "chat" => Ok(CliCommand::Chat),
        [arg] if arg == "task" => Err(CliError::new("task command is required")),
        [arg, subcommand] if arg == "task" && subcommand == "list" => {
            Ok(CliCommand::Task(TaskCommand::List))
        }
        [arg, subcommand, ..] if arg == "task" && subcommand == "list" => {
            Err(CliError::new("task list accepts no additional arguments"))
        }
        [arg, subcommand] if arg == "task" && subcommand == "show" => {
            Err(CliError::new("task id is required"))
        }
        [arg, subcommand, id] if arg == "task" && subcommand == "show" => {
            Ok(CliCommand::Task(TaskCommand::Show { id: id.to_owned() }))
        }
        [arg, subcommand, ..] if arg == "task" && subcommand == "show" => {
            Err(CliError::new("task show accepts exactly one task id"))
        }
        [arg, description @ ..] if arg == "task" => Ok(CliCommand::Task(TaskCommand::Create {
            description: description.join(" "),
        })),
        [arg] if arg == "swarm" => Ok(CliCommand::Swarm),
        [arg] if arg == "review" => Ok(CliCommand::Review),
        [arg] if arg == "diff" => Ok(CliCommand::Diff),
        [arg] if arg == "test" => Ok(CliCommand::Test),
        [arg] if arg == "merge" => Ok(CliCommand::Merge),
        [arg] if arg == "doctor" => Ok(CliCommand::Doctor),
        [unknown, ..] => Err(CliError::new(format!(
            "unknown or invalid command: {unknown}"
        ))),
    }
}

pub fn run<I, S>(args: I, current_dir: &Path) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    match parse_args(args)? {
        CliCommand::Chat => Ok(render_stub("chat")),
        CliCommand::Diff => Ok(render_stub("diff")),
        CliCommand::Doctor => Ok(render_doctor(current_dir)),
        CliCommand::Help => Ok(render_help()),
        CliCommand::Init => init_project(current_dir),
        CliCommand::Merge => Ok(render_stub("merge")),
        CliCommand::Review => Ok(render_stub("review")),
        CliCommand::Swarm => Ok(render_stub("swarm")),
        CliCommand::Task(TaskCommand::Create { description }) => {
            create_task(current_dir, &description)
        }
        CliCommand::Task(TaskCommand::List) => list_tasks(current_dir),
        CliCommand::Task(TaskCommand::Show { id }) => show_task(current_dir, &id),
        CliCommand::Test => Ok(render_stub("test")),
        CliCommand::Version => Ok(render_version()),
    }
}

pub fn render_version() -> String {
    format!("raptor {}\n", version())
}

pub fn render_help() -> String {
    format!(
        "raptor {}\n\n{}\n\nUsage:\n  raptor --version\n  raptor init\n  raptor chat\n  raptor task \"task description\"\n  raptor task list\n  raptor task show <id>\n  raptor swarm\n  raptor review\n  raptor diff\n  raptor test\n  raptor merge\n  raptor doctor\n",
        version(),
        project_description()
    )
}

pub fn init_project(current_dir: &Path) -> Result<String, CliError> {
    match init_project_config(current_dir).map_err(|error| CliError::new(error.to_string()))? {
        InitConfigOutcome::Created(path) => {
            Ok(format!("created RaptorCode config at {}\n", path.display()))
        }
        InitConfigOutcome::Exists(path) => Ok(format!(
            "RaptorCode config already exists at {}; not overwritten\n",
            path.display()
        )),
    }
}

pub fn create_task(current_dir: &Path, description: &str) -> Result<String, CliError> {
    let trimmed_description = description.trim();
    let task_id = TaskId::from_description(trimmed_description)
        .map_err(|error| CliError::new(error.to_string()))?;
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;

    if task_metadata_exists(repository_root.path(), &task_id)
        .map_err(|error| CliError::new(error.to_string()))?
    {
        return Err(CliError::new(format!(
            "task metadata already exists for {}; refusing to overwrite",
            task_id.as_str()
        )));
    }

    let plan = plan_task_worktree(&repository_root, &task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let worktree = create_task_worktree(&repository_root, &task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let metadata = TaskMetadata::new(
        task_id.clone(),
        trimmed_description,
        TaskStatus::Planned,
        worktree,
    );
    let metadata_path = persist_task_metadata(repository_root.path(), &metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(format!(
        "created task {}\nstatus: {}\nbranch: {}\nworktree: {}\nmetadata: {}\n",
        task_id.as_str(),
        TaskStatus::Planned.as_str(),
        plan.branch_name,
        plan.path.display(),
        metadata_path.display()
    ))
}

pub fn list_tasks(current_dir: &Path) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let tasks = list_task_metadata(repository_root.path())
        .map_err(|error| CliError::new(error.to_string()))?;

    if tasks.is_empty() {
        return Ok("No RaptorCode tasks found.\n".to_owned());
    }

    let mut output = String::from("ID | Status | Created | Worktree | Description\n");
    output.push_str("--- | --- | --- | --- | ---\n");

    for task in tasks {
        output.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            task.id,
            display_optional(task.status.as_deref()),
            display_optional(task.created_at.as_deref()),
            display_path_optional(task.worktree_path.as_ref()),
            display_optional(task.description.as_deref())
        ));
    }

    Ok(output)
}

pub fn show_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_task_details(&task))
}

pub fn render_doctor(current_dir: &Path) -> String {
    let repository = inspect_repository(current_dir);
    let tools = [
        command_version("git", &["--version"]),
        command_version("cargo", &["--version"]),
        command_version("rustc", &["--version"]),
    ];

    let mut output = String::new();
    output.push_str(project_name());
    output.push_str(" doctor\n");
    output.push_str(&format!("version: {}\n", version()));
    output.push_str(&format!("current_dir: {}\n", current_dir.display()));
    output.push_str(&format!(
        "repository: {}\n",
        if repository.inside_work_tree {
            "detected"
        } else {
            "not detected"
        }
    ));

    if let Some(branch) = repository.branch.as_deref() {
        output.push_str(&format!("branch: {branch}\n"));
    }

    if let Some(git_dir) = repository.git_dir.as_deref() {
        output.push_str(&format!("git_dir: {git_dir}\n"));
    }

    for tool in tools {
        output.push_str(&format!(
            "tool.{}: {} ({})\n",
            tool.name,
            if tool.available {
                "available"
            } else {
                "unavailable"
            },
            tool.detail
        ));
    }

    output
}

fn render_stub(command: &str) -> String {
    format!("raptor {command}: not implemented yet\n")
}

fn render_task_details(task: &StoredTaskMetadata) -> String {
    format!(
        "Task: {}\nStatus: {}\nDescription: {}\nCreated: {}\nBranch: {}\nRepository root: {}\nWorktree: {}\nMetadata: {}\n",
        task.id,
        display_optional(task.status.as_deref()),
        display_optional(task.description.as_deref()),
        display_optional(task.created_at.as_deref()),
        display_optional(task.branch.as_deref()),
        display_path_optional(task.repository_root.as_ref()),
        display_path_optional(task.worktree_path.as_ref()),
        task.metadata_path.display()
    )
}

fn display_optional(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or("-")
}

fn display_path_optional(value: Option<&std::path::PathBuf>) -> String {
    value
        .map(|path| path.display().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "-".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use raptor_core::TaskId;
    use raptor_store::CONFIG_FILE_NAME;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
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
                .join(format!("raptor-cli-{name}-{}-{nonce}", std::process::id()));

            fs::create_dir_all(&path).unwrap();

            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn initialize_git_repo(path: &Path) {
        run_git(path, &["init"]);
        run_git(
            path,
            &[
                "-c",
                "user.name=Raptor Test",
                "-c",
                "user.email=raptor@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
    }

    fn run_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn parses_version_flag() {
        assert_eq!(parse_args(["--version"]).unwrap(), CliCommand::Version);
    }

    #[test]
    fn parses_router_commands() {
        let commands = [
            ("init", CliCommand::Init),
            ("chat", CliCommand::Chat),
            ("swarm", CliCommand::Swarm),
            ("review", CliCommand::Review),
            ("diff", CliCommand::Diff),
            ("test", CliCommand::Test),
            ("merge", CliCommand::Merge),
            ("doctor", CliCommand::Doctor),
        ];

        for (arg, expected) in commands {
            assert_eq!(parse_args([arg]).unwrap(), expected);
        }
    }

    #[test]
    fn parses_task_description() {
        assert_eq!(
            parse_args(["task", "add", "router", "tests"]).unwrap(),
            CliCommand::Task(TaskCommand::Create {
                description: "add router tests".to_owned()
            })
        );
    }

    #[test]
    fn parses_task_list_and_show() {
        assert_eq!(
            parse_args(["task", "list"]).unwrap(),
            CliCommand::Task(TaskCommand::List)
        );
        assert_eq!(
            parse_args(["task", "show", "task-123"]).unwrap(),
            CliCommand::Task(TaskCommand::Show {
                id: "task-123".to_owned()
            })
        );
    }

    #[test]
    fn rejects_missing_task_command() {
        let error = parse_args(["task"]).unwrap_err();

        assert!(error.to_string().contains("task command"));
    }

    #[test]
    fn rejects_missing_task_show_id() {
        let error = parse_args(["task", "show"]).unwrap_err();

        assert!(error.to_string().contains("task id"));
    }

    #[test]
    fn rejects_extra_task_list_arguments() {
        let error = parse_args(["task", "list", "extra"]).unwrap_err();

        assert!(error.to_string().contains("no additional arguments"));
    }

    #[test]
    fn rejects_unknown_command() {
        let error = parse_args(["unknown"]).unwrap_err();
        assert!(error.to_string().contains("unknown"));
    }

    #[test]
    fn renders_version_output() {
        assert_eq!(render_version(), "raptor 0.1.0\n");
    }

    #[test]
    fn doctor_includes_core_diagnostics() {
        let current_dir = std::env::current_dir().unwrap();
        let output = run(["doctor"], &current_dir).unwrap();

        assert!(output.contains("RaptorCode doctor"));
        assert!(output.contains("repository:"));
        assert!(output.contains("tool.git:"));
        assert!(output.contains("tool.cargo:"));
        assert!(output.contains("tool.rustc:"));
    }

    #[test]
    fn init_creates_config_when_missing() {
        let test_dir = TestDir::create("init-create");
        let output = run(["init"], &test_dir.path).unwrap();
        let config_path = test_dir.path.join(CONFIG_FILE_NAME);

        assert!(output.contains("created RaptorCode config"));
        assert!(config_path.exists());
    }

    #[test]
    fn init_refuses_to_overwrite_existing_config() {
        let test_dir = TestDir::create("init-exists");
        let config_path = test_dir.path.join(CONFIG_FILE_NAME);
        fs::write(&config_path, "custom = true\n").unwrap();

        let output = run(["init"], &test_dir.path).unwrap();

        assert!(output.contains("not overwritten"));
        assert_eq!(fs::read_to_string(config_path).unwrap(), "custom = true\n");
    }

    #[test]
    fn task_creates_worktree_and_metadata() {
        let test_dir = TestDir::create("task");
        initialize_git_repo(&test_dir.path);
        let description = "Add task workflow tests";
        let task_id = TaskId::from_description(description).unwrap();

        let output = run(["task", description], &test_dir.path).unwrap();

        assert!(output.contains(&format!("created task {}", task_id.as_str())));
        assert!(test_dir
            .path
            .join(".raptor")
            .join("worktrees")
            .join(task_id.as_str())
            .is_dir());
        assert!(test_dir
            .path
            .join(".raptor")
            .join("tasks")
            .join(format!("{}.toml", task_id.as_str()))
            .exists());
    }

    #[test]
    fn task_refuses_to_overwrite_existing_metadata() {
        let test_dir = TestDir::create("task-exists");
        initialize_git_repo(&test_dir.path);
        let description = "Refuse duplicate task metadata";

        run(["task", description], &test_dir.path).unwrap();
        let error = run(["task", description], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("refusing to overwrite"));
    }

    #[test]
    fn task_list_handles_no_tasks() {
        let test_dir = TestDir::create("task-list-empty");
        initialize_git_repo(&test_dir.path);

        let output = run(["task", "list"], &test_dir.path).unwrap();

        assert_eq!(output, "No RaptorCode tasks found.\n");
    }

    #[test]
    fn task_list_shows_one_task() {
        let test_dir = TestDir::create("task-list-one");
        initialize_git_repo(&test_dir.path);
        let description = "List task metadata";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let output = run(["task", "list"], &test_dir.path).unwrap();

        assert!(output.contains("ID | Status | Created | Worktree | Description"));
        assert!(output.contains(task_id.as_str()));
        assert!(output.contains("planned"));
        assert!(output.contains(description));
    }

    #[test]
    fn task_show_existing_task() {
        let test_dir = TestDir::create("task-show");
        initialize_git_repo(&test_dir.path);
        let description = "Show task metadata";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let output = run(["task", "show", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains(&format!("Task: {}", task_id.as_str())));
        assert!(output.contains("Status: planned"));
        assert!(output.contains(&format!("Description: {description}")));
        assert!(output.contains("Worktree:"));
        assert!(output.contains("Metadata:"));
    }

    #[test]
    fn task_show_missing_task_is_clear() {
        let test_dir = TestDir::create("task-show-missing");
        initialize_git_repo(&test_dir.path);

        let error = run(["task", "show", "missing-task"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn task_show_rejects_unsafe_task_id() {
        let test_dir = TestDir::create("task-show-unsafe");
        initialize_git_repo(&test_dir.path);

        let error = run(["task", "show", "Unsafe"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn task_show_rejects_path_traversal_task_id() {
        let test_dir = TestDir::create("task-show-traversal");
        initialize_git_repo(&test_dir.path);

        let error = run(["task", "show", "../secret"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn task_list_and_show_preserve_read_only_behavior() {
        let test_dir = TestDir::create("task-readonly");
        initialize_git_repo(&test_dir.path);
        let description = "Preserve task metadata";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let tasks_dir = test_dir.path.join(".raptor").join("tasks");
        let metadata_path = tasks_dir.join(format!("{}.toml", task_id.as_str()));
        let before_content = fs::read_to_string(&metadata_path).unwrap();
        let before_entries = fs::read_dir(&tasks_dir).unwrap().count();

        run(["task", "list"], &test_dir.path).unwrap();
        run(["task", "show", task_id.as_str()], &test_dir.path).unwrap();

        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_content);
        assert_eq!(fs::read_dir(tasks_dir).unwrap().count(), before_entries);
    }

    #[test]
    fn stub_commands_are_clear() {
        let output = run(["chat"], Path::new(".")).unwrap();

        assert_eq!(output, "raptor chat: not implemented yet\n");
    }
}
