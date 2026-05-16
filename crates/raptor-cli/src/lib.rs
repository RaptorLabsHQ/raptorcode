use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::BufRead;
use std::path::Path;

use raptor_core::{project_description, project_name, version, TaskId, TaskMetadata, TaskStatus};
use raptor_git::{
    create_task_worktree, diff_task_worktree, execute_task_merge, execute_task_worktree_cleanup,
    inspect_repository, inspect_repository_root, inspect_task_worktree, plan_task_worktree,
    plan_task_worktree_cleanup, preflight_task_merge, GitError, MergeExecution, MergePreflight,
    RepositoryRoot, WorktreeCleanupPlan, WorktreeDiff, WorktreeReview,
};
use raptor_store::{
    init_project_config, list_task_metadata, persist_task_metadata, read_task_metadata,
    task_metadata_exists, update_task_status, InitConfigOutcome, StoredTaskMetadata,
    TaskStatusTransition,
};
use raptor_tools::command_version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Chat,
    Diff { task_id: String },
    Doctor,
    Help,
    Init,
    Merge(MergeCommand),
    Review { task_id: String },
    Swarm,
    Task(TaskCommand),
    Test,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeCommand {
    DryRun { task_id: String },
    Execute { task_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCommand {
    Abandon { id: String },
    Cleanup { id: String },
    Create { description: String },
    List,
    Show { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
    output: Option<String>,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            output: None,
        }
    }

    fn with_output(message: impl Into<String>, output: String) -> Self {
        Self {
            message: message.into(),
            output: Some(output),
        }
    }

    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
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
        [arg, subcommand] if arg == "task" && subcommand == "abandon" => {
            Err(CliError::new("task id is required"))
        }
        [arg, subcommand, id] if arg == "task" && subcommand == "abandon" => {
            Ok(CliCommand::Task(TaskCommand::Abandon { id: id.to_owned() }))
        }
        [arg, subcommand, ..] if arg == "task" && subcommand == "abandon" => {
            Err(CliError::new("task abandon accepts exactly one task id"))
        }
        [arg, subcommand] if arg == "task" && subcommand == "cleanup" => {
            Err(CliError::new("task id is required"))
        }
        [arg, subcommand, id] if arg == "task" && subcommand == "cleanup" => {
            Ok(CliCommand::Task(TaskCommand::Cleanup { id: id.to_owned() }))
        }
        [arg, subcommand, ..] if arg == "task" && subcommand == "cleanup" => {
            Err(CliError::new("task cleanup accepts exactly one task id"))
        }
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
        [arg] if arg == "review" => Err(CliError::new("review task id is required")),
        [arg, task_id] if arg == "review" => Ok(CliCommand::Review {
            task_id: task_id.to_owned(),
        }),
        [arg, ..] if arg == "review" => Err(CliError::new("review accepts exactly one task id")),
        [arg] if arg == "diff" => Err(CliError::new("diff task id is required")),
        [arg, task_id] if arg == "diff" => Ok(CliCommand::Diff {
            task_id: task_id.to_owned(),
        }),
        [arg, ..] if arg == "diff" => Err(CliError::new("diff accepts exactly one task id")),
        [arg] if arg == "test" => Ok(CliCommand::Test),
        [arg] if arg == "merge" => Err(CliError::new("merge task id is required")),
        [arg, task_id] if arg == "merge" => Ok(CliCommand::Merge(MergeCommand::DryRun {
            task_id: task_id.to_owned(),
        })),
        [arg, task_id, flag] if arg == "merge" && flag == "--execute" => {
            Ok(CliCommand::Merge(MergeCommand::Execute {
                task_id: task_id.to_owned(),
            }))
        }
        [arg, ..] if arg == "merge" => Err(CliError::new(
            "merge accepts exactly one task id and optional --execute flag",
        )),
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
    run_with_merge_confirmation(args, current_dir, None)
}

pub fn run_with_merge_confirmation<I, S>(
    args: I,
    current_dir: &Path,
    confirmation_input: Option<&mut dyn BufRead>,
) -> Result<String, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    match parse_args(args)? {
        CliCommand::Chat => Ok(render_stub("chat")),
        CliCommand::Diff { task_id } => diff_task(current_dir, &task_id),
        CliCommand::Doctor => Ok(render_doctor(current_dir)),
        CliCommand::Help => Ok(render_help()),
        CliCommand::Init => init_project(current_dir),
        CliCommand::Merge(MergeCommand::DryRun { task_id }) => merge_task(current_dir, &task_id),
        CliCommand::Merge(MergeCommand::Execute { task_id }) => {
            merge_task_execute(current_dir, &task_id, confirmation_input)
        }
        CliCommand::Review { task_id } => review_task(current_dir, &task_id),
        CliCommand::Swarm => Ok(render_stub("swarm")),
        CliCommand::Task(TaskCommand::Abandon { id }) => abandon_task(current_dir, &id),
        CliCommand::Task(TaskCommand::Cleanup { id }) => {
            cleanup_task(current_dir, &id, confirmation_input)
        }
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
        "raptor {}\n\n{}\n\nUsage:\n  raptor --version\n  raptor init\n  raptor chat\n  raptor task \"task description\"\n  raptor task list\n  raptor task show <id>\n  raptor task abandon <id>\n  raptor task cleanup <id>\n  raptor swarm\n  raptor review <task-id>\n  raptor diff <task-id>\n  raptor test\n  raptor merge <task-id>\n  raptor merge <task-id> --execute\n  raptor doctor\n",
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

pub fn abandon_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let status = task.status.as_deref().unwrap_or("unknown");

    if matches!(status, "merged" | "completed") {
        return Err(CliError::new(format!(
            "task status {status} cannot be abandoned"
        )));
    }

    let updated = update_task_status(
        repository_root.path(),
        &task.id,
        TaskStatusTransition::Abandoned,
    )
    .map_err(|error| CliError::new(error.to_string()))?;

    Ok(format!(
        "Task abandoned\nTask: {}\nStatus: {}\nWorktree retained: {}\nMetadata: {}\n",
        updated.id,
        display_optional(updated.status.as_deref()),
        display_path_optional(updated.worktree_path.as_ref()),
        updated.metadata_path.display()
    ))
}

pub fn cleanup_task(
    current_dir: &Path,
    task_id: &str,
    confirmation_input: Option<&mut dyn BufRead>,
) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let status = task.status.as_deref().unwrap_or("unknown");
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let plan = plan_task_worktree_cleanup(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    if !task_status_allows_cleanup(status) {
        return Err(CliError::new(format!(
            "task status {status} is not safe for cleanup; expected merged, completed, or abandoned"
        )));
    }

    let mut output = render_cleanup_plan(&task, status, &plan);
    let confirmation_phrase = format!("CLEANUP {}", task.id);

    output.push_str(&format!("Required confirmation: {confirmation_phrase}\n"));

    let Some(input) = confirmation_input else {
        return Err(CliError::with_output(
            "task cleanup requires interactive confirmation; aborting",
            output,
        ));
    };

    let mut confirmation = String::new();
    input.read_line(&mut confirmation).map_err(|_| {
        CliError::with_output("failed to read cleanup confirmation", output.clone())
    })?;
    let confirmation = strip_line_ending(&confirmation);

    if confirmation.is_empty() {
        return Err(CliError::with_output(
            "cleanup confirmation was empty; aborting",
            output,
        ));
    }

    if confirmation != confirmation_phrase {
        return Err(CliError::with_output(
            "cleanup confirmation did not match; aborting",
            output,
        ));
    }

    output.push_str("Confirmation result: matched\n");
    match execute_task_worktree_cleanup(&plan) {
        Ok(deleted_path) => {
            output.push_str("Cleanup result: completed\n");
            output.push_str(&format!("Deleted worktree: {}\n", deleted_path.display()));
            output.push_str(&format!(
                "Task metadata retained: {}\n",
                task.metadata_path.display()
            ));
            output.push_str("Branch deletion: not performed\n");
            output.push_str("Remote push: not performed\n");
            Ok(output)
        }
        Err(error) => {
            output.push_str("Cleanup result: failed\n");
            Err(CliError::with_output(error.to_string(), output))
        }
    }
}

pub fn review_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let review = inspect_task_worktree(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_task_review(&task, &review))
}

pub fn diff_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let diff = diff_task_worktree(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_task_diff(&task, &diff))
}

pub fn merge_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let (_, task, preflight) = merge_task_preflight(current_dir, task_id)?;

    Ok(render_merge_preflight(&task, &preflight))
}

pub fn merge_task_execute(
    current_dir: &Path,
    task_id: &str,
    confirmation_input: Option<&mut dyn BufRead>,
) -> Result<String, CliError> {
    let (repository_root, task, preflight) = merge_task_preflight(current_dir, task_id)?;
    let mut output = render_merge_preflight(&task, &preflight);
    let confirmation_phrase = format!("MERGE {}", task.id);

    output.push_str(&format!("Required confirmation: {confirmation_phrase}\n"));

    let Some(input) = confirmation_input else {
        return Err(CliError::with_output(
            "merge execution requires interactive confirmation; aborting",
            output,
        ));
    };

    let mut confirmation = String::new();
    input
        .read_line(&mut confirmation)
        .map_err(|_| CliError::with_output("failed to read merge confirmation", output.clone()))?;
    let confirmation = strip_line_ending(&confirmation);

    if confirmation.is_empty() {
        return Err(CliError::with_output(
            "merge confirmation was empty; aborting",
            output,
        ));
    }

    if confirmation != confirmation_phrase {
        return Err(CliError::with_output(
            "merge confirmation did not match; aborting",
            output,
        ));
    }

    output.push_str("Confirmation result: matched\n");
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let execution = match execute_task_merge(&repository_root, &task.id, worktree_path) {
        Ok(execution) => execution,
        Err(error) => {
            output.push_str("Merge execution result: failed\n");
            if matches!(&error, GitError::MergeConflict { .. }) {
                match update_task_status(
                    repository_root.path(),
                    &task.id,
                    TaskStatusTransition::MergeConflict,
                ) {
                    Ok(updated) => output.push_str(&render_task_status_update(&updated)),
                    Err(status_error) => {
                        output.push_str(&format!("Task status update failed: {status_error}\n"))
                    }
                }
            }

            return Err(CliError::with_output(error.to_string(), output));
        }
    };
    output.push_str(&render_merge_execution(&execution));
    match update_task_status(
        repository_root.path(),
        &task.id,
        TaskStatusTransition::Merged,
    ) {
        Ok(updated) => output.push_str(&render_task_status_update(&updated)),
        Err(error) => {
            output.push_str("Task status update failed\n");
            return Err(CliError::with_output(
                format!("merge completed but failed to update task metadata: {error}"),
                output,
            ));
        }
    }

    Ok(output)
}

fn merge_task_preflight(
    current_dir: &Path,
    task_id: &str,
) -> Result<(RepositoryRoot, StoredTaskMetadata, MergePreflight), CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = read_task_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let preflight = preflight_task_merge(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok((repository_root, task, preflight))
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
        "Task: {}\nStatus: {}\nDescription: {}\nCreated: {}\nUpdated: {}\nMerged: {}\nConflict: {}\nAbandoned: {}\nBranch: {}\nRepository root: {}\nWorktree: {}\nMetadata: {}\n",
        task.id,
        display_optional(task.status.as_deref()),
        display_optional(task.description.as_deref()),
        display_optional(task.created_at.as_deref()),
        display_optional(task.updated_at.as_deref()),
        display_optional(task.merged_at.as_deref()),
        display_optional(task.conflict_at.as_deref()),
        display_optional(task.abandoned_at.as_deref()),
        display_optional(task.branch.as_deref()),
        display_path_optional(task.repository_root.as_ref()),
        display_path_optional(task.worktree_path.as_ref()),
        task.metadata_path.display()
    )
}

fn render_task_review(task: &StoredTaskMetadata, review: &WorktreeReview) -> String {
    let mut output = format!(
        "Task: {}\nStatus: {}\nDescription: {}\nWorktree: {}\n",
        task.id,
        display_optional(task.status.as_deref()),
        display_optional(task.description.as_deref()),
        review.worktree_path.display()
    );

    output.push_str("Git status:\n");
    if review.status_lines.is_empty() {
        output.push_str("  clean\n");
    } else {
        for line in &review.status_lines {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }

    output.push_str("Changed files:\n");
    if review.changed_files.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for file in &review.changed_files {
            output.push_str("  ");
            output.push_str(file);
            output.push('\n');
        }
    }

    output
}

fn render_task_diff(task: &StoredTaskMetadata, diff: &WorktreeDiff) -> String {
    let mut output = format!(
        "Task: {}\nWorktree: {}\n",
        task.id,
        diff.worktree_path.display()
    );

    output.push_str("Unstaged diff:\n");
    push_diff_section(&mut output, &diff.unstaged);
    output.push_str("Staged diff:\n");
    push_diff_section(&mut output, &diff.staged);

    output
}

fn render_merge_preflight(task: &StoredTaskMetadata, preflight: &MergePreflight) -> String {
    let mut output = format!(
        "Merge preflight dry run\nTask: {}\nStatus: {}\nDescription: {}\nTarget repository: {}\nTarget branch: {}\nWorktree: {}\nWorktree branch: {}\n",
        task.id,
        display_optional(task.status.as_deref()),
        display_optional(task.description.as_deref()),
        preflight.target_repository_path.display(),
        display_optional(preflight.target_branch.as_deref()),
        preflight.worktree_path.display(),
        display_optional(preflight.worktree_branch.as_deref())
    );

    output.push_str("Changed files:\n");
    if preflight.changed_files.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for file in &preflight.changed_files {
            output.push_str("  ");
            output.push_str(file);
            output.push('\n');
        }
    }

    output.push_str("Preflight result: passed\n");
    output.push_str("No merge was executed.\n");
    output.push_str("Next behavior: raptor merge <task-id> --execute requires typed confirmation before supervised local merge execution.\n");

    output
}

fn render_cleanup_plan(
    task: &StoredTaskMetadata,
    status: &str,
    plan: &WorktreeCleanupPlan,
) -> String {
    format!(
        "Task cleanup plan\nTask: {}\nStatus: {}\nDelete worktree: {}\nRetain metadata: {}\nBranch deletion: not performed\nRemote push: not performed\n",
        task.id,
        status,
        plan.worktree_path().display(),
        task.metadata_path.display()
    )
}

fn render_merge_execution(execution: &MergeExecution) -> String {
    let mut output = format!(
        "Merge execution result: completed\nTarget repository: {}\nTarget branch: {}\nSource branch: {}\nWorktree retained: {}\nRemote push: not performed\n",
        execution.target_repository_path.display(),
        execution.target_branch,
        execution.source_branch,
        execution.worktree_path.display()
    );

    output.push_str("Merged files:\n");
    if execution.changed_files.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for file in &execution.changed_files {
            output.push_str("  ");
            output.push_str(file);
            output.push('\n');
        }
    }

    output.push_str("Final git status:\n");
    if execution.final_status_lines.is_empty() {
        output.push_str("  clean\n");
    } else {
        for line in &execution.final_status_lines {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }

    if !execution.merge_stdout.is_empty() {
        output.push_str("Git merge output:\n");
        for line in execution.merge_stdout.lines() {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }

    if !execution.merge_stderr.is_empty() {
        output.push_str("Git merge diagnostics:\n");
        for line in execution.merge_stderr.lines() {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

fn render_task_status_update(task: &StoredTaskMetadata) -> String {
    format!(
        "Task status updated\nTask: {}\nStatus: {}\nUpdated: {}\nMetadata: {}\n",
        task.id,
        display_optional(task.status.as_deref()),
        display_optional(task.updated_at.as_deref()),
        task.metadata_path.display()
    )
}

fn push_diff_section(output: &mut String, diff: &str) {
    if diff.trim().is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    output.push_str(diff);
    if !diff.ends_with('\n') {
        output.push('\n');
    }
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

fn task_status_allows_cleanup(status: &str) -> bool {
    matches!(status, "merged" | "completed" | "abandoned")
}

fn strip_line_ending(value: &str) -> &str {
    value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use raptor_core::TaskId;
    use raptor_store::{read_task_metadata, CONFIG_FILE_NAME};
    use std::fs;
    use std::io::Cursor;
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
        run_git(path, &["config", "user.name", "Raptor Test"]);
        run_git(path, &["config", "user.email", "raptor@example.invalid"]);
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

    fn git_text(path: &Path, args: &[&str]) -> String {
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

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn commit_file(path: &Path, relative_path: &str, content: &str) {
        fs::write(path.join(relative_path), content).unwrap();
        run_git(path, &["add", relative_path]);
        run_git(
            path,
            &[
                "-c",
                "user.name=Raptor Test",
                "-c",
                "user.email=raptor@example.invalid",
                "commit",
                "-m",
                "add fixture",
            ],
        );
    }

    fn task_worktree_path(repository_root: &Path, task_id: &TaskId) -> PathBuf {
        repository_root
            .join(".raptor")
            .join("worktrees")
            .join(task_id.as_str())
    }

    fn task_metadata_path(repository_root: &Path, task_id: &TaskId) -> PathBuf {
        repository_root
            .join(".raptor")
            .join("tasks")
            .join(format!("{}.toml", task_id.as_str()))
    }

    fn replace_worktree_path_in_metadata(
        repository_root: &Path,
        task_id: &TaskId,
        replacement_path: &Path,
    ) {
        let metadata_path = task_metadata_path(repository_root, task_id);
        let worktree_path = task_worktree_path(repository_root, task_id);
        let content = fs::read_to_string(&metadata_path).unwrap();
        let updated = content.replace(
            &worktree_path.display().to_string(),
            &replacement_path.display().to_string(),
        );

        fs::write(metadata_path, updated).unwrap();
    }

    fn run_merge_execute_with_input(
        task_id: &TaskId,
        current_dir: &Path,
        input: &str,
    ) -> Result<String, CliError> {
        run_with_confirmation_input(["merge", task_id.as_str(), "--execute"], current_dir, input)
    }

    fn run_task_cleanup_with_input(
        task_id: &TaskId,
        current_dir: &Path,
        input: &str,
    ) -> Result<String, CliError> {
        run_with_confirmation_input(["task", "cleanup", task_id.as_str()], current_dir, input)
    }

    fn run_with_confirmation_input<const N: usize>(
        args: [&str; N],
        current_dir: &Path,
        input: &str,
    ) -> Result<String, CliError> {
        let mut input = Cursor::new(input.as_bytes());

        run_with_merge_confirmation(args, current_dir, Some(&mut input))
    }

    fn task_status(repository_root: &Path, task_id: &TaskId) -> String {
        read_task_metadata(repository_root, task_id.as_str())
            .unwrap()
            .status
            .unwrap()
    }

    fn changed_merge_task(name: &str, description: &str) -> (TestDir, TaskId) {
        let test_dir = TestDir::create(name);
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::write(
            task_worktree_path(&test_dir.path, &task_id).join("tracked.txt"),
            "after\n",
        )
        .unwrap();

        (test_dir, task_id)
    }

    fn committed_merge_task(name: &str, description: &str) -> (TestDir, TaskId) {
        let test_dir = TestDir::create(name);
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        commit_file(&worktree_path, "tracked.txt", "after\n");

        (test_dir, task_id)
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
            ("test", CliCommand::Test),
            ("doctor", CliCommand::Doctor),
        ];

        for (arg, expected) in commands {
            assert_eq!(parse_args([arg]).unwrap(), expected);
        }
    }

    #[test]
    fn parses_review_and_diff_task_ids() {
        assert_eq!(
            parse_args(["review", "task-123"]).unwrap(),
            CliCommand::Review {
                task_id: "task-123".to_owned()
            }
        );
        assert_eq!(
            parse_args(["diff", "task-123"]).unwrap(),
            CliCommand::Diff {
                task_id: "task-123".to_owned()
            }
        );
    }

    #[test]
    fn parses_merge_task_id() {
        assert_eq!(
            parse_args(["merge", "task-123"]).unwrap(),
            CliCommand::Merge(MergeCommand::DryRun {
                task_id: "task-123".to_owned()
            })
        );
    }

    #[test]
    fn parses_merge_execute_flag() {
        assert_eq!(
            parse_args(["merge", "task-123", "--execute"]).unwrap(),
            CliCommand::Merge(MergeCommand::Execute {
                task_id: "task-123".to_owned()
            })
        );
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
        assert_eq!(
            parse_args(["task", "abandon", "task-123"]).unwrap(),
            CliCommand::Task(TaskCommand::Abandon {
                id: "task-123".to_owned()
            })
        );
        assert_eq!(
            parse_args(["task", "cleanup", "task-123"]).unwrap(),
            CliCommand::Task(TaskCommand::Cleanup {
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
    fn rejects_missing_review_and_diff_task_ids() {
        let review_error = parse_args(["review"]).unwrap_err();
        let diff_error = parse_args(["diff"]).unwrap_err();

        assert!(review_error.to_string().contains("task id"));
        assert!(diff_error.to_string().contains("task id"));
    }

    #[test]
    fn rejects_extra_review_and_diff_arguments() {
        let review_error = parse_args(["review", "task-123", "extra"]).unwrap_err();
        let diff_error = parse_args(["diff", "task-123", "extra"]).unwrap_err();

        assert!(review_error.to_string().contains("exactly one"));
        assert!(diff_error.to_string().contains("exactly one"));
    }

    #[test]
    fn rejects_missing_and_extra_merge_task_ids() {
        let missing_error = parse_args(["merge"]).unwrap_err();
        let extra_error = parse_args(["merge", "task-123", "extra"]).unwrap_err();

        assert!(missing_error.to_string().contains("task id"));
        assert!(extra_error.to_string().contains("optional --execute"));
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
    fn task_abandon_marks_task_without_deleting_worktree_or_metadata() {
        let test_dir = TestDir::create("task-abandon");
        initialize_git_repo(&test_dir.path);
        let description = "Abandon task";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        let metadata_path = task_metadata_path(&test_dir.path, &task_id);
        let output = run(["task", "abandon", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("Task abandoned"));
        assert_eq!(task_status(&test_dir.path, &task_id), "abandoned");
        assert!(worktree_path.exists());
        assert!(metadata_path.exists());
    }

    #[test]
    fn review_existing_task_with_clean_worktree() {
        let test_dir = TestDir::create("review-clean");
        initialize_git_repo(&test_dir.path);
        let description = "Review clean worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let output = run(["review", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains(&format!("Task: {}", task_id.as_str())));
        assert!(output.contains("Status: planned"));
        assert!(output.contains(&format!("Description: {description}")));
        assert!(output.contains("Git status:"));
        assert!(output.contains("  clean"));
        assert!(output.contains("Changed files:"));
        assert!(output.contains("  (none)"));
    }

    #[test]
    fn review_existing_task_with_changed_file() {
        let test_dir = TestDir::create("review-changed");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Review changed worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::write(
            task_worktree_path(&test_dir.path, &task_id).join("tracked.txt"),
            "after\n",
        )
        .unwrap();
        let output = run(["review", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains(" M tracked.txt"));
        assert!(output.contains("Changed files:"));
        assert!(output.contains("tracked.txt"));
    }

    #[test]
    fn diff_existing_task_with_unstaged_changed_file() {
        let test_dir = TestDir::create("diff-unstaged");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Diff changed worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::write(
            task_worktree_path(&test_dir.path, &task_id).join("tracked.txt"),
            "after\n",
        )
        .unwrap();
        let output = run(["diff", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("Unstaged diff:"));
        assert!(output.contains("diff --git"));
        assert!(output.contains("-before"));
        assert!(output.contains("+after"));
        assert!(output.contains("Staged diff:"));
        assert!(output.contains("  (none)"));
    }

    #[test]
    fn diff_existing_task_with_staged_changed_file() {
        let test_dir = TestDir::create("diff-staged");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Diff staged worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        fs::write(worktree_path.join("tracked.txt"), "after\n").unwrap();
        run_git(&worktree_path, &["add", "tracked.txt"]);
        let output = run(["diff", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("Unstaged diff:"));
        assert!(output.contains("  (none)"));
        assert!(output.contains("Staged diff:"));
        assert!(output.contains("diff --git"));
        assert!(output.contains("-before"));
        assert!(output.contains("+after"));
    }

    #[test]
    fn review_and_diff_reject_unsafe_task_ids() {
        let test_dir = TestDir::create("review-unsafe");
        initialize_git_repo(&test_dir.path);

        let review_error = run(["review", "Unsafe"], &test_dir.path).unwrap_err();
        let diff_error = run(["diff", "Unsafe"], &test_dir.path).unwrap_err();

        assert!(review_error.to_string().contains("not safe"));
        assert!(diff_error.to_string().contains("not safe"));
    }

    #[test]
    fn review_and_diff_reject_path_traversal_task_ids() {
        let test_dir = TestDir::create("review-traversal");
        initialize_git_repo(&test_dir.path);

        let review_error = run(["review", "../secret"], &test_dir.path).unwrap_err();
        let diff_error = run(["diff", "../secret"], &test_dir.path).unwrap_err();

        assert!(review_error.to_string().contains("not safe"));
        assert!(diff_error.to_string().contains("not safe"));
    }

    #[test]
    fn review_rejects_missing_worktree() {
        let test_dir = TestDir::create("review-missing-worktree");
        initialize_git_repo(&test_dir.path);
        let description = "Review missing worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::remove_dir_all(task_worktree_path(&test_dir.path, &task_id)).unwrap();
        let error = run(["review", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("worktree not found"));
    }

    #[test]
    fn review_and_diff_preserve_read_only_behavior() {
        let test_dir = TestDir::create("review-readonly");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Read only review";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        fs::write(worktree_path.join("tracked.txt"), "after\n").unwrap();
        run_git(&worktree_path, &["add", "tracked.txt"]);
        let metadata_path = task_metadata_path(&test_dir.path, &task_id);
        let before_metadata = fs::read_to_string(&metadata_path).unwrap();
        let before_status = git_text(&worktree_path, &["status", "--short"]);
        let before_unstaged_diff = git_text(&worktree_path, &["diff"]);
        let before_staged_diff = git_text(&worktree_path, &["diff", "--cached"]);

        run(["review", task_id.as_str()], &test_dir.path).unwrap();
        run(["diff", task_id.as_str()], &test_dir.path).unwrap();

        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_metadata);
        assert_eq!(
            git_text(&worktree_path, &["status", "--short"]),
            before_status
        );
        assert_eq!(git_text(&worktree_path, &["diff"]), before_unstaged_diff);
        assert_eq!(
            git_text(&worktree_path, &["diff", "--cached"]),
            before_staged_diff
        );
    }

    #[test]
    fn merge_preflight_succeeds_with_changed_worktree() {
        let test_dir = TestDir::create("merge-preflight-success");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Merge changed worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::write(
            task_worktree_path(&test_dir.path, &task_id).join("tracked.txt"),
            "after\n",
        )
        .unwrap();
        let output = run(["merge", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("Merge preflight dry run"));
        assert!(output.contains(&format!("Task: {}", task_id.as_str())));
        assert!(output.contains("Status: planned"));
        assert!(output.contains(&format!("Description: {description}")));
        assert!(output.contains("Target repository:"));
        assert!(output.contains("Target branch:"));
        assert!(output.contains("Worktree:"));
        assert!(output.contains("Worktree branch:"));
        assert!(output.contains("tracked.txt"));
        assert!(output.contains("No merge was executed."));
        assert!(output.contains("typed confirmation"));
    }

    #[test]
    fn merge_without_execute_remains_dry_run_and_does_not_prompt() {
        let (test_dir, task_id) = changed_merge_task("merge-dry-run-no-prompt", "Merge dry run");

        let output = run(["merge", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("Merge preflight dry run"));
        assert!(output.contains("No merge was executed."));
        assert!(!output.contains(&format!(
            "Required confirmation: MERGE {}",
            task_id.as_str()
        )));
        assert!(!output.contains("Merge execution is not implemented yet."));
    }

    #[test]
    fn merge_execute_prints_required_confirmation_phrase() {
        let (test_dir, task_id) =
            committed_merge_task("merge-execute-phrase", "Merge execute phrase");
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let output = run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap();

        assert!(output.contains("Merge preflight dry run"));
        assert!(output.contains(&format!(
            "Required confirmation: MERGE {}",
            task_id.as_str()
        )));
    }

    #[test]
    fn merge_execute_aborts_on_empty_input() {
        let (test_dir, task_id) = changed_merge_task("merge-execute-empty", "Merge empty input");

        let error = run_merge_execute_with_input(&task_id, &test_dir.path, "\n").unwrap_err();

        assert!(error.to_string().contains("empty"));
        assert!(error.output().unwrap().contains("Merge preflight dry run"));
        assert!(error.output().unwrap().contains(&format!(
            "Required confirmation: MERGE {}",
            task_id.as_str()
        )));
    }

    #[test]
    fn merge_execute_aborts_on_wrong_phrase() {
        let (test_dir, task_id) = changed_merge_task("merge-execute-wrong", "Merge wrong phrase");

        let error = run_merge_execute_with_input(&task_id, &test_dir.path, "MERGE another-task\n")
            .unwrap_err();

        assert!(error.to_string().contains("did not match"));
        assert!(error.output().unwrap().contains(&format!(
            "Required confirmation: MERGE {}",
            task_id.as_str()
        )));
    }

    #[test]
    fn merge_execute_merges_simple_committed_task_after_exact_confirmation() {
        let (test_dir, task_id) = committed_merge_task("merge-execute-simple", "Merge simple task");
        let confirmation = format!("MERGE {}\n", task_id.as_str());
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);

        let output = run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap();

        assert!(output.contains("Confirmation result: matched"));
        assert!(output.contains("Merge execution result: completed"));
        assert!(output.contains("Merged files:"));
        assert!(output.contains("tracked.txt"));
        assert!(output.contains("Remote push: not performed"));
        assert_eq!(
            fs::read_to_string(test_dir.path.join("tracked.txt")).unwrap(),
            "after\n"
        );
        assert_eq!(
            git_text(
                &test_dir.path,
                &["status", "--short", "--untracked-files=no"]
            ),
            ""
        );
        assert!(worktree_path.exists());
        assert_eq!(task_status(&test_dir.path, &task_id), "merged");
    }

    #[test]
    fn merge_execute_reports_conflict_without_auto_resolution() {
        let test_dir = TestDir::create("merge-execute-conflict");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "base\n");
        let description = "Merge execute conflict";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        commit_file(&worktree_path, "tracked.txt", "task\n");
        commit_file(&test_dir.path, "tracked.txt", "target\n");
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(
            error.to_string().contains("merge conflicts detected"),
            "unexpected error: {error}"
        );
        assert!(error
            .output()
            .unwrap()
            .contains("Merge execution result: failed"));
        assert!(git_text(&test_dir.path, &["status", "--short"]).contains("UU tracked.txt"));
        assert!(worktree_path.exists());
        assert_eq!(task_status(&test_dir.path, &task_id), "merge_conflict");
    }

    #[test]
    fn merge_execute_rejects_uncommitted_task_worktree_after_confirmation() {
        let (test_dir, task_id) =
            changed_merge_task("merge-execute-uncommitted", "Merge uncommitted task");
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("uncommitted changes"));
        assert!(error
            .output()
            .unwrap()
            .contains("Confirmation result: matched"));
        assert!(error
            .output()
            .unwrap()
            .contains("Merge execution result: failed"));
        assert_eq!(task_status(&test_dir.path, &task_id), "planned");
    }

    #[test]
    fn merge_execute_rejects_missing_interactive_input() {
        let (test_dir, task_id) =
            changed_merge_task("merge-execute-noninteractive", "Merge no input");

        let error = run_with_merge_confirmation(
            ["merge", task_id.as_str(), "--execute"],
            &test_dir.path,
            None,
        )
        .unwrap_err();

        assert!(error.to_string().contains("interactive confirmation"));
        assert!(error.output().unwrap().contains(&format!(
            "Required confirmation: MERGE {}",
            task_id.as_str()
        )));
    }

    #[test]
    fn merge_execute_rejects_dirty_target_repository() {
        let (test_dir, task_id) =
            committed_merge_task("merge-execute-dirty-root", "Merge dirty target");
        fs::write(test_dir.path.join("root-dirty.txt"), "dirty\n").unwrap();
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("uncommitted changes"));
    }

    #[test]
    fn merge_execute_failed_preflight_does_not_mark_merged() {
        let test_dir = TestDir::create("merge-execute-preflight-status");
        initialize_git_repo(&test_dir.path);
        let description = "Merge preflight status";
        let task_id = TaskId::from_description(description).unwrap();
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        run(["task", description], &test_dir.path).unwrap();
        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("no changes to merge"));
        assert_eq!(task_status(&test_dir.path, &task_id), "planned");
    }

    #[test]
    fn merge_execute_rejects_missing_worktree() {
        let (test_dir, task_id) = committed_merge_task(
            "merge-execute-missing-worktree",
            "Merge missing execute worktree",
        );
        fs::remove_dir_all(task_worktree_path(&test_dir.path, &task_id)).unwrap();
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("worktree not found"));
    }

    #[test]
    fn merge_execute_rejects_unsafe_worktree_path() {
        let (test_dir, task_id) = committed_merge_task(
            "merge-execute-unsafe-worktree",
            "Merge unsafe execute worktree",
        );
        let unsafe_path = test_dir.path.join("outside").join(task_id.as_str());
        replace_worktree_path_in_metadata(&test_dir.path, &task_id, &unsafe_path);
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("not a safe repository path"));
    }

    #[test]
    fn merge_execute_rejects_source_branch_equal_to_target_branch() {
        let test_dir = TestDir::create("merge-execute-same-branch");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Merge same branch";
        let task_id = TaskId::from_description(description).unwrap();
        let target_branch = git_text(&test_dir.path, &["branch", "--show-current"])
            .trim()
            .to_owned();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        run_git(
            &worktree_path,
            &["checkout", "--ignore-other-worktrees", &target_branch],
        );
        fs::write(worktree_path.join("tracked.txt"), "after\n").unwrap();
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let error =
            run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap_err();

        assert!(error.to_string().contains("same"));
        assert!(error
            .output()
            .unwrap()
            .contains("Confirmation result: matched"));
    }

    #[test]
    fn merge_preflight_rejects_missing_task() {
        let test_dir = TestDir::create("merge-missing-task");
        initialize_git_repo(&test_dir.path);

        let error = run(["merge", "missing-task"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn merge_preflight_rejects_unsafe_task_id() {
        let test_dir = TestDir::create("merge-unsafe");
        initialize_git_repo(&test_dir.path);

        let error = run(["merge", "Unsafe"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn merge_preflight_rejects_path_traversal_task_id() {
        let test_dir = TestDir::create("merge-traversal");
        initialize_git_repo(&test_dir.path);

        let error = run(["merge", "../secret"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not safe"));
    }

    #[test]
    fn merge_preflight_rejects_missing_worktree() {
        let test_dir = TestDir::create("merge-missing-worktree");
        initialize_git_repo(&test_dir.path);
        let description = "Merge missing worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::remove_dir_all(task_worktree_path(&test_dir.path, &task_id)).unwrap();
        let error = run(["merge", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("worktree not found"));
    }

    #[test]
    fn merge_preflight_rejects_worktree_outside_raptor_directory() {
        let test_dir = TestDir::create("merge-unsafe-worktree");
        initialize_git_repo(&test_dir.path);
        let description = "Merge unsafe worktree";
        let task_id = TaskId::from_description(description).unwrap();
        let unsafe_path = test_dir.path.join("outside").join(task_id.as_str());

        run(["task", description], &test_dir.path).unwrap();
        replace_worktree_path_in_metadata(&test_dir.path, &task_id, &unsafe_path);
        let error = run(["merge", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not a safe repository path"));
    }

    #[test]
    fn merge_preflight_rejects_dirty_root_repository() {
        let test_dir = TestDir::create("merge-dirty-root");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Merge dirty root";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        fs::write(
            task_worktree_path(&test_dir.path, &task_id).join("tracked.txt"),
            "after\n",
        )
        .unwrap();
        fs::write(test_dir.path.join("root-dirty.txt"), "dirty\n").unwrap();
        let error = run(["merge", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("uncommitted changes"));
    }

    #[test]
    fn merge_preflight_rejects_clean_task_worktree() {
        let test_dir = TestDir::create("merge-clean-worktree");
        initialize_git_repo(&test_dir.path);
        let description = "Merge clean worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let error = run(["merge", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("no changes to merge"));
    }

    #[test]
    fn merge_preflight_rejects_unresolved_conflict_state() {
        let test_dir = TestDir::create("merge-conflict");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "base\n");
        let description = "Merge conflicted worktree";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);

        fs::write(test_dir.path.join("tracked.txt"), "target\n").unwrap();
        run_git(&test_dir.path, &["add", "tracked.txt"]);
        run_git(
            &test_dir.path,
            &[
                "-c",
                "user.name=Raptor Test",
                "-c",
                "user.email=raptor@example.invalid",
                "commit",
                "-m",
                "target change",
            ],
        );
        let target_head = git_text(&test_dir.path, &["rev-parse", "HEAD"])
            .trim()
            .to_owned();
        commit_file(&worktree_path, "tracked.txt", "task\n");

        let merge_output = Command::new("git")
            .args([
                "-c",
                "user.name=Raptor Test",
                "-c",
                "user.email=raptor@example.invalid",
                "merge",
                target_head.as_str(),
            ])
            .current_dir(&worktree_path)
            .output()
            .unwrap();

        assert!(
            !merge_output.status.success(),
            "conflict setup unexpectedly merged cleanly"
        );
        assert!(
            !git_text(&worktree_path, &["status", "--short"]).is_empty(),
            "merge stdout: {}; merge stderr: {}",
            String::from_utf8_lossy(&merge_output.stdout),
            String::from_utf8_lossy(&merge_output.stderr)
        );

        let error = run(["merge", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(
            error.to_string().contains("unresolved conflicts"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn merge_preflight_has_no_merge_stage_commit_or_push_side_effects() {
        let test_dir = TestDir::create("merge-readonly");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "before\n");
        let description = "Merge readonly";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        fs::write(worktree_path.join("tracked.txt"), "after\n").unwrap();
        run_git(&worktree_path, &["add", "tracked.txt"]);
        let metadata_path = task_metadata_path(&test_dir.path, &task_id);
        let before_metadata = fs::read_to_string(&metadata_path).unwrap();
        let before_root_head = git_text(&test_dir.path, &["rev-parse", "HEAD"]);
        let before_worktree_head = git_text(&worktree_path, &["rev-parse", "HEAD"]);
        let before_root_status = git_text(&test_dir.path, &["status", "--short"]);
        let before_worktree_status = git_text(&worktree_path, &["status", "--short"]);
        let before_worktree_staged_diff = git_text(&worktree_path, &["diff", "--cached"]);

        let output = run(["merge", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("No merge was executed."));
        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_metadata);
        assert_eq!(
            git_text(&test_dir.path, &["rev-parse", "HEAD"]),
            before_root_head
        );
        assert_eq!(
            git_text(&worktree_path, &["rev-parse", "HEAD"]),
            before_worktree_head
        );
        assert_eq!(
            git_text(&test_dir.path, &["status", "--short"]),
            before_root_status
        );
        assert_eq!(
            git_text(&worktree_path, &["status", "--short"]),
            before_worktree_status
        );
        assert_eq!(
            git_text(&worktree_path, &["diff", "--cached"]),
            before_worktree_staged_diff
        );
    }

    #[test]
    fn merge_execute_does_not_push_and_keeps_worktree_after_success() {
        let (test_dir, task_id) =
            committed_merge_task("merge-execute-no-push", "Merge without push");
        let remote_dir = TestDir::create("merge-execute-remote");
        run_git(&remote_dir.path, &["init", "--bare"]);
        run_git(
            &test_dir.path,
            &["remote", "add", "origin", remote_dir.path.to_str().unwrap()],
        );
        let target_branch = git_text(&test_dir.path, &["branch", "--show-current"])
            .trim()
            .to_owned();
        run_git(&test_dir.path, &["push", "origin", &target_branch]);
        let remote_ref = format!("refs/heads/{target_branch}");
        let remote_before = git_text(&test_dir.path, &["ls-remote", "origin", &remote_ref]);
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        let confirmation = format!("MERGE {}\n", task_id.as_str());

        let output = run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap();

        assert!(output.contains("Merge execution result: completed"));
        assert!(worktree_path.exists());
        assert_eq!(task_status(&test_dir.path, &task_id), "merged");
        assert_eq!(
            git_text(
                &test_dir.path,
                &["status", "--short", "--untracked-files=no"]
            ),
            ""
        );
        assert_eq!(
            git_text(&test_dir.path, &["ls-remote", "origin", &remote_ref]),
            remote_before
        );
    }

    #[test]
    fn cleanup_rejects_missing_task() {
        let test_dir = TestDir::create("cleanup-missing-task");
        initialize_git_repo(&test_dir.path);

        let error = run(["task", "cleanup", "missing-task"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn cleanup_rejects_unsafe_and_path_traversal_task_ids() {
        let test_dir = TestDir::create("cleanup-unsafe-id");
        initialize_git_repo(&test_dir.path);

        let unsafe_error = run(["task", "cleanup", "Unsafe"], &test_dir.path).unwrap_err();
        let traversal_error = run(["task", "cleanup", "../secret"], &test_dir.path).unwrap_err();

        assert!(unsafe_error.to_string().contains("not safe"));
        assert!(traversal_error.to_string().contains("not safe"));
    }

    #[test]
    fn cleanup_rejects_active_task() {
        let test_dir = TestDir::create("cleanup-active");
        initialize_git_repo(&test_dir.path);
        let description = "Cleanup active task";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let error = run(["task", "cleanup", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not safe for cleanup"));
        assert!(task_worktree_path(&test_dir.path, &task_id).exists());
    }

    #[test]
    fn cleanup_rejects_unsafe_worktree_path() {
        let (test_dir, task_id) =
            committed_merge_task("cleanup-unsafe-worktree", "Cleanup unsafe worktree");
        let confirmation = format!("MERGE {}\n", task_id.as_str());
        run_merge_execute_with_input(&task_id, &test_dir.path, &confirmation).unwrap();
        let unsafe_path = test_dir.path.join("outside").join(task_id.as_str());
        replace_worktree_path_in_metadata(&test_dir.path, &task_id, &unsafe_path);
        let cleanup_confirmation = format!("CLEANUP {}\n", task_id.as_str());

        let error = run_task_cleanup_with_input(&task_id, &test_dir.path, &cleanup_confirmation)
            .unwrap_err();

        assert!(error.to_string().contains("not a safe repository path"));
    }

    #[test]
    fn cleanup_aborts_on_missing_or_wrong_confirmation() {
        let (test_dir, task_id) =
            committed_merge_task("cleanup-confirmation", "Cleanup confirmation");
        let merge_confirmation = format!("MERGE {}\n", task_id.as_str());
        run_merge_execute_with_input(&task_id, &test_dir.path, &merge_confirmation).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);

        let missing_error = run_with_merge_confirmation(
            ["task", "cleanup", task_id.as_str()],
            &test_dir.path,
            None,
        )
        .unwrap_err();
        let wrong_error =
            run_task_cleanup_with_input(&task_id, &test_dir.path, "CLEANUP wrong\n").unwrap_err();

        assert!(missing_error
            .to_string()
            .contains("interactive confirmation"));
        assert!(missing_error
            .output()
            .unwrap()
            .contains("Task cleanup plan"));
        assert!(wrong_error.to_string().contains("did not match"));
        assert!(worktree_path.exists());
    }

    #[test]
    fn cleanup_deletes_only_expected_worktree_and_preserves_metadata_branch_and_remote() {
        let (test_dir, task_id) =
            committed_merge_task("cleanup-success", "Cleanup successful task");
        let remote_dir = TestDir::create("cleanup-remote");
        run_git(&remote_dir.path, &["init", "--bare"]);
        run_git(
            &test_dir.path,
            &["remote", "add", "origin", remote_dir.path.to_str().unwrap()],
        );
        let target_branch = git_text(&test_dir.path, &["branch", "--show-current"])
            .trim()
            .to_owned();
        run_git(&test_dir.path, &["push", "origin", &target_branch]);
        let remote_ref = format!("refs/heads/{target_branch}");
        let remote_before = git_text(&test_dir.path, &["ls-remote", "origin", &remote_ref]);
        let merge_confirmation = format!("MERGE {}\n", task_id.as_str());
        run_merge_execute_with_input(&task_id, &test_dir.path, &merge_confirmation).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        let metadata_path = task_metadata_path(&test_dir.path, &task_id);
        let branch_name = format!("raptor-task-{}", task_id.as_str());
        let branch_ref = format!("refs/heads/{branch_name}");
        run_git(
            &test_dir.path,
            &["show-ref", "--verify", "--quiet", &branch_ref],
        );
        let sibling = test_dir
            .path
            .join(".raptor")
            .join("worktrees")
            .join("sibling");
        fs::create_dir_all(&sibling).unwrap();
        let cleanup_confirmation = format!("CLEANUP {}\n", task_id.as_str());

        let output =
            run_task_cleanup_with_input(&task_id, &test_dir.path, &cleanup_confirmation).unwrap();

        assert!(output.contains("Task cleanup plan"));
        assert!(output.contains("Cleanup result: completed"));
        assert!(!worktree_path.exists());
        assert!(sibling.exists());
        assert!(metadata_path.exists());
        run_git(
            &test_dir.path,
            &["show-ref", "--verify", "--quiet", &branch_ref],
        );
        assert_eq!(
            git_text(&test_dir.path, &["ls-remote", "origin", &remote_ref]),
            remote_before
        );
    }

    #[test]
    fn cleanup_allows_abandoned_task() {
        let test_dir = TestDir::create("cleanup-abandoned");
        initialize_git_repo(&test_dir.path);
        let description = "Cleanup abandoned task";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        run(["task", "abandon", task_id.as_str()], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        let cleanup_confirmation = format!("CLEANUP {}\n", task_id.as_str());

        let output =
            run_task_cleanup_with_input(&task_id, &test_dir.path, &cleanup_confirmation).unwrap();

        assert!(output.contains("Cleanup result: completed"));
        assert!(!worktree_path.exists());
        assert!(task_metadata_path(&test_dir.path, &task_id).exists());
    }

    #[test]
    fn stub_commands_are_clear() {
        let output = run(["chat"], Path::new(".")).unwrap();

        assert_eq!(output, "raptor chat: not implemented yet\n");
    }
}
