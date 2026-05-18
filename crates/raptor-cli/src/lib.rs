use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};

use raptor_agent::{
    build_role_planning_prompt, role_prompt_template, AgentId, AgentRole, AgentSession,
    PermissionPolicy,
};
use raptor_core::{
    project_description, project_name, version, TaskId, TaskMetadata, TaskStatus,
    SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS, SWARM_IMPLEMENTATION_FAILED_STATUS,
    SWARM_PENDING_MODEL_EXECUTION_STATUS, SWARM_PLANNING_COMPLETED_STATUS,
    SWARM_PLANNING_FAILED_STATUS, SWARM_PROPOSAL_GENERATED_STATUS,
};
use raptor_git::{
    create_swarm_worktrees, create_task_worktree, diff_swarm_role_worktree, diff_task_worktree,
    execute_task_merge, execute_task_worktree_cleanup, inspect_repository, inspect_repository_root,
    inspect_swarm_role_worktree, inspect_task_worktree, plan_task_worktree,
    plan_task_worktree_cleanup, preflight_task_merge, validated_swarm_role_worktree_path,
    validated_task_worktree_path, GitError, MergeExecution, MergePreflight, RepositoryRoot,
    SwarmWorktreePlan, WorktreeCleanupPlan, WorktreeDiff, WorktreeReview,
};
use raptor_providers::{
    CompletionMessage, CompletionRequest, CompletionResponse, DeepSeekProvider, ModelId, Provider,
    ProviderCatalog, ProviderConfiguration, ProviderError, ProviderId, ProviderSummary, TokenUsage,
    DEEPSEEK_API_KEY_URL,
};
use raptor_store::{
    init_project_config, list_agent_sessions, list_task_metadata, persist_agent_session,
    persist_swarm_metadata, persist_task_metadata, read_agent_session, read_project_config,
    read_swarm_metadata, read_task_metadata, swarm_metadata_exists, task_metadata_exists,
    update_swarm_metadata, update_task_status, InitConfigOutcome, ProjectConfigError,
    StoredAgentSession, StoredTaskMetadata, SwarmAgentMetadata, SwarmMetadata,
    TaskStatusTransition, TaskStoreError,
};
use raptor_tools::{
    command_version, detect_test_command, explicit_test_command, run_test_command,
    TestCommandOutput,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Agent(AgentCommand),
    Chat {
        message: String,
    },
    Diff {
        role: Option<String>,
        task_id: String,
    },
    Doctor,
    Help,
    Init,
    Merge(MergeCommand),
    Provider(ProviderCommand),
    Review {
        role: Option<String>,
        task_id: String,
    },
    Swarm {
        description: String,
    },
    SwarmImplement {
        role: String,
        task_id: String,
    },
    Task(TaskCommand),
    Test {
        task_id: String,
        test_command: Option<Vec<String>>,
    },
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCommand {
    Create {
        model_id: Option<String>,
        provider_id: Option<String>,
        role: String,
        task_id: Option<String>,
    },
    List,
    Policy,
    Show {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeCommand {
    DryRun { task_id: String },
    Execute { task_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderCommand {
    List,
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

const SWARM_PROVIDER_ID: &str = "deepseek";
const SWARM_PROVIDER_DISPLAY: &str = "DeepSeek";
const SWARM_MODEL_ID: &str = "deepseek-chat";
const CHAT_SYSTEM_PROMPT: &str = "You are RaptorCode, a supervised coding assistant. Answer the developer's message directly. Do not claim to modify files, run commands, install dependencies, commit, merge, or push.";
const SWARM_PLANNING_SYSTEM_PROMPT: &str = "You are RaptorCode, a supervised v0.1 planning assistant for Raptor Labs. Produce role-specific implementation planning only. Do not modify files, execute commands, apply patches, run tests, claim work was completed, or propose Phase 2 features. DeepSeek is the only provider in scope before v0.1.";
const SWARM_IMPLEMENTATION_SYSTEM_PROMPT: &str = "You are RaptorCode, a supervised v0.1 implementation proposal assistant for Raptor Labs. Produce a safe, reviewable implementation proposal only. Do not claim files were modified. Do not execute commands. Do not apply patches. Do not instruct RaptorCode to modify source files automatically. DeepSeek is the only provider in scope before v0.1.";
const RAPTOR_PLAN_FILE_NAME: &str = "RAPTOR_PLAN.md";
const RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME: &str = "RAPTOR_IMPLEMENTATION_PROPOSAL.md";

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
        [arg] if arg == "agent" => Err(CliError::new("agent command is required")),
        [arg, subcommand, options @ ..] if arg == "agent" && subcommand == "create" => {
            parse_agent_create_options(options)
        }
        [arg, subcommand] if arg == "agent" && subcommand == "list" => {
            Ok(CliCommand::Agent(AgentCommand::List))
        }
        [arg, subcommand, ..] if arg == "agent" && subcommand == "list" => {
            Err(CliError::new("agent list accepts no additional arguments"))
        }
        [arg, subcommand] if arg == "agent" && subcommand == "policy" => {
            Ok(CliCommand::Agent(AgentCommand::Policy))
        }
        [arg, subcommand, ..] if arg == "agent" && subcommand == "policy" => Err(CliError::new(
            "agent policy accepts no additional arguments",
        )),
        [arg, subcommand] if arg == "agent" && subcommand == "show" => {
            Err(CliError::new("agent id is required"))
        }
        [arg, subcommand, id] if arg == "agent" && subcommand == "show" => {
            Ok(CliCommand::Agent(AgentCommand::Show { id: id.to_owned() }))
        }
        [arg, subcommand, ..] if arg == "agent" && subcommand == "show" => {
            Err(CliError::new("agent show accepts exactly one agent id"))
        }
        [arg] if arg == "chat" => Err(CliError::new("chat message is required")),
        [arg, message @ ..] if arg == "chat" => Ok(CliCommand::Chat {
            message: message.join(" "),
        }),
        [arg] if arg == "provider" => Err(CliError::new("provider command is required")),
        [arg, subcommand] if arg == "provider" && subcommand == "list" => {
            Ok(CliCommand::Provider(ProviderCommand::List))
        }
        [arg, subcommand, ..] if arg == "provider" && subcommand == "list" => Err(CliError::new(
            "provider list accepts no additional arguments",
        )),
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
        [arg] if arg == "swarm" => Err(CliError::new("swarm task description is required")),
        [arg, subcommand] if arg == "swarm" && subcommand == "implement" => {
            Err(CliError::new("swarm implement task id is required"))
        }
        [arg, subcommand, _task_id] if arg == "swarm" && subcommand == "implement" => {
            Err(CliError::new("swarm implement role is required"))
        }
        [arg, subcommand, task_id, role] if arg == "swarm" && subcommand == "implement" => {
            Ok(CliCommand::SwarmImplement {
                role: role.to_owned(),
                task_id: task_id.to_owned(),
            })
        }
        [arg, subcommand, ..] if arg == "swarm" && subcommand == "implement" => Err(CliError::new(
            "swarm implement accepts exactly one task id and one role",
        )),
        [arg, description @ ..] if arg == "swarm" => Ok(CliCommand::Swarm {
            description: description.join(" "),
        }),
        [arg] if arg == "review" => Err(CliError::new("review task id is required")),
        [arg, task_id] if arg == "review" => Ok(CliCommand::Review {
            role: None,
            task_id: task_id.to_owned(),
        }),
        [arg, task_id, role] if arg == "review" => Ok(CliCommand::Review {
            role: Some(role.to_owned()),
            task_id: task_id.to_owned(),
        }),
        [arg, ..] if arg == "review" => Err(CliError::new(
            "review accepts one task id and optional swarm role",
        )),
        [arg] if arg == "diff" => Err(CliError::new("diff task id is required")),
        [arg, task_id] if arg == "diff" => Ok(CliCommand::Diff {
            role: None,
            task_id: task_id.to_owned(),
        }),
        [arg, task_id, role] if arg == "diff" => Ok(CliCommand::Diff {
            role: Some(role.to_owned()),
            task_id: task_id.to_owned(),
        }),
        [arg, ..] if arg == "diff" => Err(CliError::new(
            "diff accepts one task id and optional swarm role",
        )),
        [arg] if arg == "test" => Err(CliError::new("test task id is required")),
        [arg, task_id] if arg == "test" => Ok(CliCommand::Test {
            task_id: task_id.to_owned(),
            test_command: None,
        }),
        [arg, task_id, separator, command @ ..] if arg == "test" && separator == "--" => {
            if command.is_empty() {
                return Err(CliError::new("explicit test command is required after --"));
            }

            Ok(CliCommand::Test {
                task_id: task_id.to_owned(),
                test_command: Some(command.to_vec()),
            })
        }
        [arg, ..] if arg == "test" => Err(CliError::new(
            "test accepts one task id and optional explicit command after --",
        )),
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

fn parse_agent_create_options(options: &[String]) -> Result<CliCommand, CliError> {
    let mut model_id = None;
    let mut provider_id = None;
    let mut role = None;
    let mut task_id = None;
    let mut index = 0;

    while index < options.len() {
        let flag = options[index].as_str();
        let value = options
            .get(index + 1)
            .ok_or_else(|| CliError::new(format!("{flag} requires a value")))?;

        match flag {
            "--model" if model_id.is_none() => model_id = Some(value.to_owned()),
            "--model" => return Err(CliError::new("agent create accepts one --model value")),
            "--provider" if provider_id.is_none() => provider_id = Some(value.to_owned()),
            "--provider" => {
                return Err(CliError::new("agent create accepts one --provider value"));
            }
            "--role" if role.is_none() => role = Some(value.to_owned()),
            "--role" => return Err(CliError::new("agent create accepts one --role value")),
            "--task" if task_id.is_none() => task_id = Some(value.to_owned()),
            "--task" => return Err(CliError::new("agent create accepts one --task value")),
            _ if flag.starts_with("--") => {
                return Err(CliError::new(format!(
                    "unknown agent create option: {flag}"
                )));
            }
            _ => {
                return Err(CliError::new(format!(
                    "unexpected agent create argument: {flag}"
                )))
            }
        }

        index += 2;
    }

    let role = role.ok_or_else(|| CliError::new("agent create requires --role <role>"))?;

    Ok(CliCommand::Agent(AgentCommand::Create {
        model_id,
        provider_id,
        role,
        task_id,
    }))
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
        CliCommand::Agent(AgentCommand::Create {
            model_id,
            provider_id,
            role,
            task_id,
        }) => create_agent(current_dir, &role, task_id, provider_id, model_id),
        CliCommand::Agent(AgentCommand::List) => list_agents(current_dir),
        CliCommand::Agent(AgentCommand::Policy) => Ok(render_permission_policy(
            &PermissionPolicy::deny_by_default(),
        )),
        CliCommand::Agent(AgentCommand::Show { id }) => show_agent(current_dir, &id),
        CliCommand::Chat { message } => chat(&message),
        CliCommand::Diff { role, task_id } => diff_task(current_dir, &task_id, role.as_deref()),
        CliCommand::Doctor => Ok(render_doctor(current_dir)),
        CliCommand::Help => Ok(render_help()),
        CliCommand::Init => init_project(current_dir),
        CliCommand::Merge(MergeCommand::DryRun { task_id }) => merge_task(current_dir, &task_id),
        CliCommand::Merge(MergeCommand::Execute { task_id }) => {
            merge_task_execute(current_dir, &task_id, confirmation_input)
        }
        CliCommand::Provider(ProviderCommand::List) => list_providers(current_dir),
        CliCommand::Review { role, task_id } => review_task(current_dir, &task_id, role.as_deref()),
        CliCommand::Swarm { description } => create_swarm(current_dir, &description),
        CliCommand::SwarmImplement { role, task_id } => {
            implement_swarm_role(current_dir, &task_id, &role)
        }
        CliCommand::Task(TaskCommand::Abandon { id }) => abandon_task(current_dir, &id),
        CliCommand::Task(TaskCommand::Cleanup { id }) => {
            cleanup_task(current_dir, &id, confirmation_input)
        }
        CliCommand::Task(TaskCommand::Create { description }) => {
            create_task(current_dir, &description)
        }
        CliCommand::Task(TaskCommand::List) => list_tasks(current_dir),
        CliCommand::Task(TaskCommand::Show { id }) => show_task(current_dir, &id),
        CliCommand::Test {
            task_id,
            test_command,
        } => test_task(current_dir, &task_id, test_command.as_deref()),
        CliCommand::Version => Ok(render_version()),
    }
}

pub fn render_version() -> String {
    format!("raptor {}\n", version())
}

pub fn render_help() -> String {
    format!(
        "raptor {}\n\n{}\n\nUsage:\n  raptor --version\n  raptor init\n  raptor provider list\n  raptor agent list\n  raptor agent show <id>\n  raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]\n  raptor agent policy\n  raptor chat \"message\"\n  raptor task \"task description\"\n  raptor task list\n  raptor task show <id>\n  raptor task abandon <id>\n  raptor task cleanup <id>\n  raptor swarm \"task description\"\n  raptor swarm implement <task-id> <role>\n  raptor review <task-id> [role]\n  raptor diff <task-id> [role]\n  raptor test <task-id> [-- <command>]\n  raptor merge <task-id>\n  raptor merge <task-id> --execute\n  raptor doctor\n",
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

pub fn list_providers(current_dir: &Path) -> Result<String, CliError> {
    let config_dir = project_config_dir(current_dir);
    let catalog = ProviderCatalog::built_in();
    let (config_status, configurations) = match read_project_config(&config_dir) {
        Ok(config) => {
            let configurations = config
                .providers
                .iter()
                .map(|provider| {
                    ProviderId::new(provider.id.clone())
                        .map(|provider_id| {
                            ProviderConfiguration::new(provider_id, provider.enabled)
                        })
                        .map_err(|error| CliError::new(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;

            (
                format!("configured ({})", config.path.display()),
                configurations,
            )
        }
        Err(ProjectConfigError::NotFound(path)) => (
            format!("not configured ({} not found)", path.display()),
            Vec::new(),
        ),
        Err(error) => return Err(CliError::new(error.to_string())),
    };
    let summaries = catalog.summaries(&configurations);

    Ok(render_provider_list(&config_status, &summaries))
}

pub fn chat(message: &str) -> Result<String, CliError> {
    let provider =
        DeepSeekProvider::from_env().map_err(|error| CliError::new(error.to_string()))?;

    chat_with_provider(message, &provider)
}

pub fn chat_with_provider(message: &str, provider: &dyn Provider) -> Result<String, CliError> {
    let message = message.trim();

    if message.is_empty() {
        return Err(CliError::new("chat message is required"));
    }

    if provider.id().as_str() != "deepseek" {
        return Err(CliError::new(
            "only the DeepSeek provider is implemented before v0.1",
        ));
    }

    let model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model configured"))?;
    let request = CompletionRequest::new(
        provider.id().clone(),
        model_id,
        vec![
            CompletionMessage::system(CHAT_SYSTEM_PROMPT),
            CompletionMessage::user(message),
        ],
    );
    let response = provider
        .complete(request)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_chat_response(&response))
}

pub fn create_agent(
    current_dir: &Path,
    role: &str,
    task_id: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let role = AgentRole::parse(role).map_err(|error| CliError::new(error.to_string()))?;
    let provider_id = provider_id
        .map(|provider_id| {
            ProviderId::new(provider_id)
                .map(|provider_id| provider_id.as_str().to_owned())
                .map_err(|error| CliError::new(error.to_string()))
        })
        .transpose()?;
    let model_id = model_id
        .map(|model_id| {
            ModelId::new(model_id)
                .map(|model_id| model_id.as_str().to_owned())
                .map_err(|error| CliError::new(error.to_string()))
        })
        .transpose()?;
    let task = task_id
        .as_deref()
        .map(|task_id| {
            read_task_metadata(repository_root.path(), task_id)
                .map_err(|error| CliError::new(error.to_string()))
        })
        .transpose()?;
    let agent_id = AgentId::from_session_parts(
        role,
        task_id.as_deref(),
        provider_id.as_deref(),
        model_id.as_deref(),
    )
    .map_err(|error| CliError::new(error.to_string()))?;
    let mut session = AgentSession::new_read_only(agent_id, role);

    if let Some(provider_id) = provider_id {
        session
            .set_provider_id(provider_id)
            .map_err(|error| CliError::new(error.to_string()))?;
    }
    if let Some(model_id) = model_id {
        session
            .set_model_id(model_id)
            .map_err(|error| CliError::new(error.to_string()))?;
    }
    if let Some(task) = task.as_ref() {
        let worktree_path = task
            .worktree_path
            .clone()
            .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
        session
            .bind_task(task.id.clone(), worktree_path)
            .map_err(|error| CliError::new(error.to_string()))?;
    }

    let metadata_path = persist_agent_session(repository_root.path(), &session)
        .map_err(|error| CliError::new(error.to_string()))?;
    let stored = read_agent_session(repository_root.path(), session.id.as_str())
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_agent_created(&stored, &metadata_path))
}

pub fn list_agents(current_dir: &Path) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let sessions = list_agent_sessions(repository_root.path())
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_agent_list(&sessions))
}

pub fn show_agent(current_dir: &Path, agent_id: &str) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let session = read_agent_session(repository_root.path(), agent_id)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_agent_session(&session))
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

pub fn create_swarm(current_dir: &Path, description: &str) -> Result<String, CliError> {
    create_swarm_with_planning_source(current_dir, description, SwarmPlanningSource::Environment)
}

#[cfg_attr(not(test), allow(dead_code))]
enum SwarmPlanningSource<'a> {
    Environment,
    MissingDeepSeekApiKey,
    Provider(&'a dyn Provider),
}

#[cfg_attr(not(test), allow(dead_code))]
enum SwarmImplementationSource<'a> {
    Environment,
    MissingDeepSeekApiKey,
    Provider(&'a dyn Provider),
}

struct SwarmRoleReview<'a> {
    agent: &'a SwarmAgentMetadata,
    review: WorktreeReview,
}

struct SwarmRoleDiff<'a> {
    agent: &'a SwarmAgentMetadata,
    diff: WorktreeDiff,
}

struct TestTarget {
    label: String,
    worktree_path: PathBuf,
}

struct TestTargetResult {
    label: String,
    output: TestCommandOutput,
}

fn create_swarm_with_planning_source(
    current_dir: &Path,
    description: &str,
    planning_source: SwarmPlanningSource<'_>,
) -> Result<String, CliError> {
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

    if swarm_metadata_exists(repository_root.path(), &task_id)
        .map_err(|error| CliError::new(error.to_string()))?
    {
        return Err(CliError::new(format!(
            "swarm metadata already exists for {}; refusing to overwrite",
            task_id.as_str()
        )));
    }

    let plan = create_swarm_worktrees(&repository_root, &task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let mut metadata = swarm_metadata_from_plan(&plan, trimmed_description);
    let metadata_path = persist_swarm_metadata(repository_root.path(), &metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    match planning_source {
        SwarmPlanningSource::Environment => match DeepSeekProvider::from_env() {
            Ok(provider) => plan_swarm_agents(
                &repository_root,
                &task_id,
                trimmed_description,
                &mut metadata,
                &provider,
            )?,
            Err(ProviderError::MissingApiKey) => mark_swarm_planning_missing_api_key(&mut metadata),
            Err(error) => mark_swarm_planning_failed(&mut metadata, Some(error.to_string())),
        },
        SwarmPlanningSource::MissingDeepSeekApiKey => {
            mark_swarm_planning_missing_api_key(&mut metadata)
        }
        SwarmPlanningSource::Provider(provider) => plan_swarm_agents(
            &repository_root,
            &task_id,
            trimmed_description,
            &mut metadata,
            provider,
        )?,
    }

    update_swarm_metadata(repository_root.path(), &metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_swarm_created(&metadata, &metadata_path))
}

fn swarm_metadata_from_plan(plan: &SwarmWorktreePlan, description: &str) -> SwarmMetadata {
    let agents = plan
        .agents
        .iter()
        .map(|agent| SwarmAgentMetadata {
            agent_id: format!("{}-{}", plan.task_id, agent.role),
            branch: agent.branch_name.clone(),
            implementation_status: None,
            model: Some(SWARM_MODEL_ID.to_owned()),
            plan_file_path: None,
            planning_status: Some(SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned()),
            proposal_file_path: None,
            provider: Some(SWARM_PROVIDER_ID.to_owned()),
            role: agent.role.clone(),
            status: SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned(),
            worktree_path: agent.path.clone(),
        })
        .collect();

    SwarmMetadata {
        agents,
        created_at: None,
        description: description.to_owned(),
        id: plan.task_id.clone(),
        model: SWARM_MODEL_ID.to_owned(),
        planning_status: Some(SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned()),
        provider: SWARM_PROVIDER_ID.to_owned(),
        status: SWARM_PENDING_MODEL_EXECUTION_STATUS.to_owned(),
        updated_at: None,
    }
}

fn plan_swarm_agents(
    repository_root: &RepositoryRoot,
    task_id: &TaskId,
    task_description: &str,
    metadata: &mut SwarmMetadata,
    provider: &dyn Provider,
) -> Result<(), CliError> {
    if provider.id().as_str() != SWARM_PROVIDER_ID {
        return Err(CliError::new(
            "only the DeepSeek provider is implemented before v0.1",
        ));
    }

    let model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model configured"))?;
    let mut failed_roles = Vec::new();

    for agent in &mut metadata.agents {
        let prompt = build_role_planning_prompt(task_id.as_str(), task_description, &agent.role)
            .map_err(|error| CliError::new(error.to_string()))?;
        let request = CompletionRequest::new(
            provider.id().clone(),
            model_id.clone(),
            vec![
                CompletionMessage::system(SWARM_PLANNING_SYSTEM_PROMPT),
                CompletionMessage::user(prompt),
            ],
        );

        match provider.complete(request) {
            Ok(response) => {
                let plan_file_path = plan_file_path_for_agent(
                    repository_root.path(),
                    task_id.as_str(),
                    &agent.role,
                    &agent.worktree_path,
                )?;
                let rendered_plan =
                    render_raptor_plan(task_id.as_str(), task_description, &agent.role, &response);

                if let Err(error) = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&plan_file_path)
                    .and_then(|mut file| {
                        use std::io::Write;

                        file.write_all(rendered_plan.as_bytes())
                    })
                {
                    agent.status = SWARM_PLANNING_FAILED_STATUS.to_owned();
                    agent.planning_status = Some(SWARM_PLANNING_FAILED_STATUS.to_owned());
                    agent.plan_file_path = None;
                    failed_roles.push(format!("{}: {}", agent.role, error));
                    continue;
                }

                agent.status = SWARM_PLANNING_COMPLETED_STATUS.to_owned();
                agent.planning_status = Some(SWARM_PLANNING_COMPLETED_STATUS.to_owned());
                agent.plan_file_path = Some(plan_file_path);
                agent.provider = Some(response.provider_id.as_str().to_owned());
                agent.model = Some(response.model_id.as_str().to_owned());
            }
            Err(error) => {
                agent.status = SWARM_PLANNING_FAILED_STATUS.to_owned();
                agent.planning_status = Some(SWARM_PLANNING_FAILED_STATUS.to_owned());
                agent.plan_file_path = None;
                failed_roles.push(format!("{}: {}", agent.role, error));
            }
        }
    }

    if failed_roles.is_empty() {
        metadata.status = SWARM_PLANNING_COMPLETED_STATUS.to_owned();
        metadata.planning_status = Some(SWARM_PLANNING_COMPLETED_STATUS.to_owned());
    } else {
        metadata.status = SWARM_PLANNING_FAILED_STATUS.to_owned();
        metadata.planning_status = Some(SWARM_PLANNING_FAILED_STATUS.to_owned());
    }

    Ok(())
}

fn mark_swarm_planning_missing_api_key(metadata: &mut SwarmMetadata) {
    metadata.status = SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned();
    metadata.planning_status = Some(SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned());

    for agent in &mut metadata.agents {
        agent.status = SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned();
        agent.planning_status = Some(SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned());
        agent.plan_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.model = Some(SWARM_MODEL_ID.to_owned());
    }
}

fn mark_swarm_planning_failed(metadata: &mut SwarmMetadata, _detail: Option<String>) {
    metadata.status = SWARM_PLANNING_FAILED_STATUS.to_owned();
    metadata.planning_status = Some(SWARM_PLANNING_FAILED_STATUS.to_owned());

    for agent in &mut metadata.agents {
        agent.status = SWARM_PLANNING_FAILED_STATUS.to_owned();
        agent.planning_status = Some(SWARM_PLANNING_FAILED_STATUS.to_owned());
        agent.plan_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.model = Some(SWARM_MODEL_ID.to_owned());
    }
}

fn plan_file_path_for_agent(
    repository_root: &Path,
    task_id: &str,
    role: &str,
    worktree_path: &Path,
) -> Result<PathBuf, CliError> {
    validate_swarm_agent_worktree_path(repository_root, task_id, role, worktree_path)?;

    let plan_file_path = worktree_path.join(RAPTOR_PLAN_FILE_NAME);

    if plan_file_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::new(format!(
            "unsafe plan file path: {}",
            plan_file_path.display()
        )));
    }

    Ok(plan_file_path)
}

fn proposal_file_path_for_agent(
    repository_root: &Path,
    task_id: &str,
    role: &str,
    worktree_path: &Path,
) -> Result<PathBuf, CliError> {
    validate_swarm_agent_worktree_path(repository_root, task_id, role, worktree_path)?;

    let proposal_file_path = worktree_path.join(RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME);

    if proposal_file_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::new(format!(
            "unsafe proposal file path: {}",
            proposal_file_path.display()
        )));
    }

    Ok(proposal_file_path)
}

fn validate_swarm_agent_worktree_path(
    repository_root: &Path,
    task_id: &str,
    role: &str,
    worktree_path: &Path,
) -> Result<(), CliError> {
    let expected_worktree_path = repository_root
        .join(".raptor")
        .join("worktrees")
        .join(task_id)
        .join(role);

    if worktree_path != expected_worktree_path
        || worktree_path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(CliError::new(format!(
            "unsafe swarm worktree path: {}",
            worktree_path.display()
        )));
    }

    Ok(())
}

fn render_raptor_plan(
    task_id: &str,
    task_description: &str,
    role: &str,
    response: &CompletionResponse,
) -> String {
    format!(
        "# RaptorCode Agent Plan\n\nTask id: {task_id}\nTask description: {task_description}\nAgent role: {role}\nProvider: {}\nModel: {}\n\n## Generated Plan\n\n{}\n\n## Safety Note\n\nThis plan is AI-generated and must be reviewed before implementation. Do not execute model output automatically.\n",
        SWARM_PROVIDER_DISPLAY,
        response.model_id.as_str(),
        response.content.trim()
    )
}

pub fn implement_swarm_role(
    current_dir: &Path,
    task_id: &str,
    role: &str,
) -> Result<String, CliError> {
    implement_swarm_role_with_source(
        current_dir,
        task_id,
        role,
        SwarmImplementationSource::Environment,
    )
}

fn implement_swarm_role_with_source(
    current_dir: &Path,
    task_id: &str,
    role: &str,
    implementation_source: SwarmImplementationSource<'_>,
) -> Result<String, CliError> {
    role_prompt_template(role).map_err(|error| CliError::new(error.to_string()))?;

    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let mut metadata = read_swarm_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let agent_index = metadata
        .agents
        .iter()
        .position(|agent| agent.role == role)
        .ok_or_else(|| CliError::new(format!("swarm role not found for task {task_id}: {role}")))?;
    let worktree_path = metadata.agents[agent_index].worktree_path.clone();

    validate_swarm_agent_worktree_path(repository_root.path(), task_id, role, &worktree_path)?;
    if !worktree_path.is_dir() {
        return Err(CliError::new(format!(
            "swarm role worktree not found: {}",
            worktree_path.display()
        )));
    }

    match implementation_source {
        SwarmImplementationSource::Environment => match DeepSeekProvider::from_env() {
            Ok(provider) => generate_swarm_implementation_proposal(
                repository_root.path(),
                task_id,
                role,
                &mut metadata,
                agent_index,
                &provider,
            ),
            Err(ProviderError::MissingApiKey) => {
                mark_swarm_role_implementation_missing_api_key(
                    repository_root.path(),
                    &mut metadata,
                    agent_index,
                )?;
                Ok(render_swarm_implementation_blocked(
                    &metadata,
                    &metadata.agents[agent_index],
                ))
            }
            Err(error) => {
                mark_swarm_role_implementation_failed(
                    repository_root.path(),
                    &mut metadata,
                    agent_index,
                )?;
                Err(CliError::new(error.to_string()))
            }
        },
        SwarmImplementationSource::MissingDeepSeekApiKey => {
            mark_swarm_role_implementation_missing_api_key(
                repository_root.path(),
                &mut metadata,
                agent_index,
            )?;
            Ok(render_swarm_implementation_blocked(
                &metadata,
                &metadata.agents[agent_index],
            ))
        }
        SwarmImplementationSource::Provider(provider) => generate_swarm_implementation_proposal(
            repository_root.path(),
            task_id,
            role,
            &mut metadata,
            agent_index,
            provider,
        ),
    }
}

fn generate_swarm_implementation_proposal(
    repository_root: &Path,
    task_id: &str,
    role: &str,
    metadata: &mut SwarmMetadata,
    agent_index: usize,
    provider: &dyn Provider,
) -> Result<String, CliError> {
    if provider.id().as_str() != SWARM_PROVIDER_ID {
        return Err(CliError::new(
            "only the DeepSeek provider is implemented before v0.1",
        ));
    }

    let agent = metadata.agents[agent_index].clone();
    let plan_file_path = agent
        .plan_file_path
        .clone()
        .unwrap_or_else(|| agent.worktree_path.join(RAPTOR_PLAN_FILE_NAME));

    if agent.planning_status.as_deref() == Some(SWARM_PLANNING_COMPLETED_STATUS)
        && !plan_file_path.is_file()
    {
        return Err(CliError::new(format!(
            "RAPTOR_PLAN.md is missing for completed planning role {role}: {}",
            plan_file_path.display()
        )));
    }
    if !plan_file_path.is_file() {
        return Err(CliError::new(format!(
            "RAPTOR_PLAN.md is required before implementation proposal generation: {}",
            plan_file_path.display()
        )));
    }

    let plan_content = fs::read_to_string(&plan_file_path)
        .map_err(|source| CliError::new(format!("{}: {source}", plan_file_path.display())))?;
    let model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model configured"))?;
    let prompt = build_swarm_implementation_prompt(&metadata.description, role, &plan_content);
    let request = CompletionRequest::new(
        provider.id().clone(),
        model_id,
        vec![
            CompletionMessage::system(SWARM_IMPLEMENTATION_SYSTEM_PROMPT),
            CompletionMessage::user(prompt),
        ],
    );
    let response = match provider.complete(request) {
        Ok(response) => response,
        Err(error) => {
            mark_swarm_role_implementation_failed(repository_root, metadata, agent_index)?;
            return Err(CliError::new(error.to_string()));
        }
    };
    let proposal_file_path =
        proposal_file_path_for_agent(repository_root, task_id, role, &agent.worktree_path)?;
    let proposal = render_swarm_implementation_proposal(task_id, role, &response);

    if let Err(error) = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&proposal_file_path)
        .and_then(|mut file| {
            use std::io::Write;

            file.write_all(proposal.as_bytes())
        })
    {
        mark_swarm_role_implementation_failed(repository_root, metadata, agent_index)?;
        return Err(CliError::new(format!(
            "{}: {error}",
            proposal_file_path.display()
        )));
    }

    {
        let agent = &mut metadata.agents[agent_index];
        agent.implementation_status = Some(SWARM_PROPOSAL_GENERATED_STATUS.to_owned());
        agent.model = Some(response.model_id.as_str().to_owned());
        agent.proposal_file_path = Some(proposal_file_path.clone());
        agent.provider = Some(response.provider_id.as_str().to_owned());
        agent.status = SWARM_PROPOSAL_GENERATED_STATUS.to_owned();
    }

    update_swarm_metadata(repository_root, metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_swarm_implementation_generated(
        metadata,
        &metadata.agents[agent_index],
    ))
}

fn mark_swarm_role_implementation_missing_api_key(
    repository_root: &Path,
    metadata: &mut SwarmMetadata,
    agent_index: usize,
) -> Result<(), CliError> {
    {
        let agent = &mut metadata.agents[agent_index];
        agent.implementation_status =
            Some(SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned());
        agent.model = Some(SWARM_MODEL_ID.to_owned());
        agent.proposal_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.status = SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS.to_owned();
    }

    update_swarm_metadata(repository_root, metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(())
}

fn mark_swarm_role_implementation_failed(
    repository_root: &Path,
    metadata: &mut SwarmMetadata,
    agent_index: usize,
) -> Result<(), CliError> {
    {
        let agent = &mut metadata.agents[agent_index];
        agent.implementation_status = Some(SWARM_IMPLEMENTATION_FAILED_STATUS.to_owned());
        agent.model = Some(SWARM_MODEL_ID.to_owned());
        agent.proposal_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.status = SWARM_IMPLEMENTATION_FAILED_STATUS.to_owned();
    }

    update_swarm_metadata(repository_root, metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(())
}

fn build_swarm_implementation_prompt(
    task_description: &str,
    role: &str,
    plan_content: &str,
) -> String {
    format!(
        "RaptorCode v0.1 supervised implementation proposal request\n\nTask description:\n{}\n\nSelected role: {role}\n\nRole planning artifact:\n{}\n\nReturn a concise Markdown implementation proposal. Include recommended files to inspect or modify, manual next steps, risks, and any patch-like content only as text. Do not ask RaptorCode to apply patches. Do not execute commands. Do not modify files. Do not include repository contents beyond the plan above. Do not propose Phase 2 features.",
        task_description.trim(),
        plan_content.trim()
    )
}

fn render_swarm_implementation_proposal(
    task_id: &str,
    role: &str,
    response: &CompletionResponse,
) -> String {
    format!(
        "# RaptorCode Implementation Proposal\n\nTask id: {task_id}\nRole: {role}\nProvider: {}\nModel: {}\n\n## Generated Implementation Proposal\n\n{}\n\n## Recommended Files To Inspect Or Modify\n\nReview the generated proposal above and inspect only the files it identifies before making manual source changes.\n\n## Manual Next Steps\n\n1. Review this proposal.\n2. Inspect the referenced files manually.\n3. Apply changes only after developer approval.\n4. Use review, diff, and test workflows before merge.\n\n## Safety Note\n\nThis generated output is untrusted and must be reviewed before applying to source code. RaptorCode did not execute model output, apply patches, run commands, or modify application source files.\n",
        SWARM_PROVIDER_DISPLAY,
        response.model_id.as_str(),
        response.content.trim()
    )
}

fn render_swarm_implementation_generated(
    metadata: &SwarmMetadata,
    agent: &SwarmAgentMetadata,
) -> String {
    let proposal_file_path = agent
        .proposal_file_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "-".to_owned());

    format!(
        "RaptorCode Implementation Proposal Generated\nTask: {}\nRole: {}\nWorktree: {}\nProposal file: {}\nProvider: {}\nModel: {}\nStatus: {}\nNext commands:\n  Inspect proposal file: {}\n  raptor diff {} {}\n  raptor review {} {}\n  raptor test {}\nSource code auto-editing: not implemented\nModel output execution: not performed\nPolicy enforcement hooks: not implemented\n",
        metadata.id,
        agent.role,
        agent.worktree_path.display(),
        proposal_file_path,
        agent.provider.as_deref().unwrap_or(SWARM_PROVIDER_ID),
        agent.model.as_deref().unwrap_or(SWARM_MODEL_ID),
        agent
            .implementation_status
            .as_deref()
            .unwrap_or(&agent.status),
        proposal_file_path,
        metadata.id,
        agent.role,
        metadata.id,
        agent.role,
        metadata.id
    )
}

fn render_swarm_implementation_blocked(
    metadata: &SwarmMetadata,
    agent: &SwarmAgentMetadata,
) -> String {
    format!(
        "RaptorCode Implementation Proposal Blocked\nTask: {}\nRole: {}\nWorktree: {}\nProvider: {}\nModel: {}\nStatus: {}\nGet a DeepSeek API key at {DEEPSEEK_API_KEY_URL}\nProposal file: not written\nSource code auto-editing: not implemented\nModel output execution: not performed\n",
        metadata.id,
        agent.role,
        agent.worktree_path.display(),
        agent.provider.as_deref().unwrap_or(SWARM_PROVIDER_ID),
        agent.model.as_deref().unwrap_or(SWARM_MODEL_ID),
        agent
            .implementation_status
            .as_deref()
            .unwrap_or(&agent.status)
    )
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

pub fn review_task(
    current_dir: &Path,
    task_id: &str,
    role: Option<&str>,
) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = match read_task_metadata(repository_root.path(), task_id) {
        Ok(task) => task,
        Err(TaskStoreError::NotFound(_)) => {
            return review_swarm_task(&repository_root, task_id, role);
        }
        Err(error) => return Err(CliError::new(error.to_string())),
    };

    if role.is_some() {
        return Err(CliError::new(
            "role selection is supported only for swarm review",
        ));
    }

    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let review = inspect_task_worktree(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_task_review(&task, &review))
}

pub fn diff_task(
    current_dir: &Path,
    task_id: &str,
    role: Option<&str>,
) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let task = match read_task_metadata(repository_root.path(), task_id) {
        Ok(task) => task,
        Err(TaskStoreError::NotFound(_)) => {
            return diff_swarm_task(&repository_root, task_id, role);
        }
        Err(error) => return Err(CliError::new(error.to_string())),
    };

    if role.is_some() {
        return Err(CliError::new(
            "role selection is supported only for swarm diff",
        ));
    }

    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let diff = diff_task_worktree(&repository_root, &task.id, worktree_path)
        .map_err(|error| CliError::new(error.to_string()))?;

    Ok(render_task_diff(&task, &diff))
}

pub fn test_task(
    current_dir: &Path,
    task_id: &str,
    explicit_command_parts: Option<&[String]>,
) -> Result<String, CliError> {
    let repository_root =
        inspect_repository_root(current_dir).map_err(|error| CliError::new(error.to_string()))?;
    let explicit_command = explicit_command_parts
        .map(explicit_test_command)
        .transpose()
        .map_err(|error| CliError::new(error.to_string()))?;
    let (mode, targets) = test_targets(&repository_root, task_id)?;

    let mut results = Vec::new();
    for target in targets {
        let command = match explicit_command.as_ref() {
            Some(command) => command.clone(),
            None => match detect_test_command(&target.worktree_path) {
                Some(command) => command,
                None => {
                    let output = render_test_command_not_detected(task_id, mode, &target);
                    return Err(CliError::with_output(
                        "no test command detected; pass an explicit test command after --",
                        output,
                    ));
                }
            },
        };
        let output = run_test_command(&target.worktree_path, &command)
            .map_err(|error| CliError::new(error.to_string()))?;

        results.push(TestTargetResult {
            label: target.label,
            output,
        });
    }

    let rendered = render_test_run(task_id, mode, &results);

    if results.iter().all(|result| result.output.success) {
        Ok(rendered)
    } else {
        Err(CliError::with_output("test command failed", rendered))
    }
}

fn test_targets(
    repository_root: &RepositoryRoot,
    task_id: &str,
) -> Result<(&'static str, Vec<TestTarget>), CliError> {
    match read_task_metadata(repository_root.path(), task_id) {
        Ok(task) => {
            let worktree_path = task
                .worktree_path
                .as_ref()
                .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
            let worktree_path =
                validated_task_worktree_path(repository_root, &task.id, worktree_path)
                    .map_err(|error| CliError::new(error.to_string()))?;

            Ok((
                "single-task",
                vec![TestTarget {
                    label: task.id,
                    worktree_path,
                }],
            ))
        }
        Err(TaskStoreError::NotFound(_)) => {
            let metadata = read_swarm_metadata(repository_root.path(), task_id)
                .map_err(|error| CliError::new(error.to_string()))?;
            let mut targets = Vec::new();

            for agent in &metadata.agents {
                role_prompt_template(&agent.role)
                    .map_err(|error| CliError::new(error.to_string()))?;
                let worktree_path = validated_swarm_role_worktree_path(
                    repository_root,
                    &metadata.id,
                    &agent.role,
                    &agent.worktree_path,
                )
                .map_err(|error| CliError::new(error.to_string()))?;
                targets.push(TestTarget {
                    label: agent.role.clone(),
                    worktree_path,
                });
            }
            if targets.is_empty() {
                return Err(CliError::new("swarm metadata has no test targets"));
            }

            Ok(("swarm", targets))
        }
        Err(error) => Err(CliError::new(error.to_string())),
    }
}

fn review_swarm_task(
    repository_root: &RepositoryRoot,
    task_id: &str,
    role: Option<&str>,
) -> Result<String, CliError> {
    if let Some(role) = role {
        role_prompt_template(role).map_err(|error| CliError::new(error.to_string()))?;
    }

    let metadata = read_swarm_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let agents = selected_swarm_agents(&metadata, role)?;
    let mut reviews = Vec::new();

    for agent in agents {
        let review = inspect_swarm_role_worktree(
            repository_root,
            &metadata.id,
            &agent.role,
            &agent.worktree_path,
        )
        .map_err(|error| CliError::new(error.to_string()))?;
        reviews.push(SwarmRoleReview { agent, review });
    }

    Ok(render_swarm_review(&metadata, role, &reviews))
}

fn diff_swarm_task(
    repository_root: &RepositoryRoot,
    task_id: &str,
    role: Option<&str>,
) -> Result<String, CliError> {
    if let Some(role) = role {
        role_prompt_template(role).map_err(|error| CliError::new(error.to_string()))?;
    }

    let metadata = read_swarm_metadata(repository_root.path(), task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let agents = selected_swarm_agents(&metadata, role)?;
    let mut diffs = Vec::new();

    for agent in agents {
        let diff = diff_swarm_role_worktree(
            repository_root,
            &metadata.id,
            &agent.role,
            &agent.worktree_path,
        )
        .map_err(|error| CliError::new(error.to_string()))?;
        diffs.push(SwarmRoleDiff { agent, diff });
    }

    Ok(render_swarm_diff(&metadata, role, &diffs))
}

fn selected_swarm_agents<'a>(
    metadata: &'a SwarmMetadata,
    role: Option<&str>,
) -> Result<Vec<&'a SwarmAgentMetadata>, CliError> {
    let Some(role) = role else {
        return Ok(metadata.agents.iter().collect());
    };

    role_prompt_template(role).map_err(|error| CliError::new(error.to_string()))?;
    metadata
        .agents
        .iter()
        .find(|agent| agent.role == role)
        .map(|agent| vec![agent])
        .ok_or_else(|| {
            CliError::new(format!(
                "swarm role not found for task {}: {role}",
                metadata.id
            ))
        })
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

fn project_config_dir(current_dir: &Path) -> PathBuf {
    inspect_repository_root(current_dir)
        .map(|repository_root| repository_root.path().to_path_buf())
        .unwrap_or_else(|_| current_dir.to_path_buf())
}

fn render_provider_list(config_status: &str, providers: &[ProviderSummary]) -> String {
    let mut output = format!("Provider configuration: {config_status}\n");

    if providers.is_empty() {
        output.push_str("No RaptorCode providers registered.\n");
        output.push_str("No external requests, model calls, or API integrations are enabled.\n");
        return output;
    }

    output.push_str("Provider | Name | Status | Registered | Default model | Capabilities\n");
    output.push_str("--- | --- | --- | --- | --- | ---\n");

    for provider in providers {
        let status = if provider.enabled {
            "enabled"
        } else if provider.configured {
            "disabled"
        } else {
            "not configured"
        };
        let default_model = provider
            .default_model
            .as_ref()
            .map(|model| model.as_str())
            .unwrap_or("-");
        let capabilities = provider.capabilities.labels();
        let capabilities = if capabilities.is_empty() {
            "-".to_owned()
        } else {
            capabilities.join(", ")
        };

        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {}\n",
            provider.provider_id.as_str(),
            provider.display_name,
            status,
            if provider.registered { "yes" } else { "no" },
            default_model,
            capabilities
        ));
    }

    output.push_str("Execution: disabled; provider list performs no network or model calls.\n");
    output
}

fn render_agent_created(stored: &StoredAgentSession, metadata_path: &Path) -> String {
    let session = &stored.session;

    format!(
        "Agent session created\nAgent: {}\nRole: {}\nState: {}\nExecution mode: {}\nProvider: {}\nModel: {}\nTask: {}\nMetadata: {}\nModel execution: not performed\nShell execution: not performed\nSource file writes: not performed\n",
        session.id.as_str(),
        session.role.as_str(),
        session.state.as_str(),
        session.execution_mode.as_str(),
        display_optional(session.provider_id.as_deref()),
        display_optional(session.model_id.as_deref()),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.task_id.as_str())
            .unwrap_or("-"),
        metadata_path.display()
    )
}

fn render_agent_list(sessions: &[StoredAgentSession]) -> String {
    if sessions.is_empty() {
        return "No RaptorCode agent sessions found.\nExecution: read-only foundation only; no model calls, shell execution, or file modification are configured.\n".to_owned();
    }

    let mut output = String::from(
        "ID | Role | State | Execution mode | Created | Updated | Provider | Model | Task\n",
    );
    output.push_str("--- | --- | --- | --- | --- | --- | --- | --- | ---\n");

    for stored in sessions {
        let session = &stored.session;
        output.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {} | {}\n",
            session.id.as_str(),
            session.role.as_str(),
            session.state.as_str(),
            session.execution_mode.as_str(),
            display_optional(session.created_at.as_deref()),
            display_optional(session.updated_at.as_deref()),
            display_optional(session.provider_id.as_deref()),
            display_optional(session.model_id.as_deref()),
            session
                .task_binding
                .as_ref()
                .map(|binding| binding.task_id.as_str())
                .unwrap_or("-")
        ));
    }

    output
}

fn render_agent_session(stored: &StoredAgentSession) -> String {
    let session = &stored.session;

    format!(
        "Agent: {}\nRole: {}\nState: {}\nExecution mode: {}\nCreated: {}\nUpdated: {}\nProvider: {}\nModel: {}\nTask: {}\nWorktree: {}\nMessages: {}\nEvents: {}\nAudit entries: {}\nAudit summary: {}\nMetadata: {}\nModel execution: not performed\nShell execution: not performed\nSource file writes: not performed\n",
        session.id.as_str(),
        session.role.as_str(),
        session.state.as_str(),
        session.execution_mode.as_str(),
        display_optional(session.created_at.as_deref()),
        display_optional(session.updated_at.as_deref()),
        display_optional(session.provider_id.as_deref()),
        display_optional(session.model_id.as_deref()),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.task_id.as_str())
            .unwrap_or("-"),
        session
            .task_binding
            .as_ref()
            .map(|binding| binding.worktree_path.display().to_string())
            .unwrap_or_else(|| "-".to_owned()),
        session.messages.len(),
        session.events.len(),
        session.audit_entries.len(),
        display_optional(session.audit_summary.as_deref()),
        stored.metadata_path.display()
    )
}

fn render_permission_policy(policy: &PermissionPolicy) -> String {
    format!(
        "RaptorCode default permission policy\nApproval mode: {}\nFile read: {}\nFile write: {}\nShell execution: {}\nNetwork: {}\nModel execution: {}\nDangerous commands: {}\nFile write allowed: {}\nShell execution allowed: {}\nNetwork allowed: {}\nModel execution allowed: {}\nExecution: policy display only; no model calls, shell commands, network requests, or source file writes are performed.\n",
        policy.approval_mode.as_str(),
        policy.file_read.as_str(),
        policy.file_write.as_str(),
        policy.shell_execution.as_str(),
        policy.network.as_str(),
        policy.model_execution.as_str(),
        policy.dangerous_commands.as_str(),
        if policy.allows_file_write() { "yes" } else { "no" },
        if policy.allows_shell_execution() { "yes" } else { "no" },
        if policy.allows_network() { "yes" } else { "no" },
        if policy.allows_model_execution() { "yes" } else { "no" }
    )
}

fn render_chat_response(response: &CompletionResponse) -> String {
    let mut output = format!(
        "DeepSeek response\nProvider: {}\nModel: {}\nFinish reason: {}\nUsage: {}\n\n{}\n",
        response.provider_id.as_str(),
        response.model_id.as_str(),
        display_optional(response.finish_reason.as_deref()),
        render_token_usage(response.usage.as_ref()),
        response.content
    );

    output.push_str("Model output execution: not performed\n");
    output.push_str("Source file writes: not performed\n");
    output.push_str("Shell execution: not performed\n");
    output
}

fn render_token_usage(usage: Option<&TokenUsage>) -> String {
    let Some(usage) = usage else {
        return "-".to_owned();
    };

    format!(
        "prompt={} completion={} total={}",
        display_number_optional(usage.prompt_tokens),
        display_number_optional(usage.completion_tokens),
        display_number_optional(usage.total_tokens)
    )
}

fn render_swarm_created(metadata: &SwarmMetadata, metadata_path: &Path) -> String {
    let mut output = format!(
        "RaptorCode Swarm Created\nTask: {}\nDescription: {}\nProvider: {}\nModel: {}\nStatus: {}\nPlanning status: {}\nMetadata: {}\n",
        metadata.id,
        metadata.description,
        SWARM_PROVIDER_DISPLAY,
        metadata.model,
        metadata.status,
        metadata.planning_status.as_deref().unwrap_or("-"),
        metadata_path.display()
    );

    output.push_str("Agents:\n");
    for agent in &metadata.agents {
        output.push_str(&format!(
            "  {} | branch: {} | worktree: {} | plan: {}",
            agent.role,
            agent.branch,
            agent.worktree_path.display(),
            agent.planning_status.as_deref().unwrap_or(&agent.status)
        ));

        if let Some(plan_file_path) = agent.plan_file_path.as_ref() {
            output.push_str(&format!(" | plan file: {}", plan_file_path.display()));
        }

        output.push('\n');
    }

    if metadata.planning_status.as_deref() == Some(SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS) {
        output.push_str(&format!(
            "DeepSeek API key required: Get a DeepSeek API key at {DEEPSEEK_API_KEY_URL}\n"
        ));
    }

    output.push_str("Next commands:\n");
    output.push_str(&format!("  raptor review {}\n", metadata.id));
    output.push_str(&format!("  raptor review {} <role>\n", metadata.id));
    output.push_str(&format!("  raptor diff {}\n", metadata.id));
    output.push_str(&format!("  raptor diff {} <role>\n", metadata.id));
    output.push_str(&format!("  raptor test {}\n", metadata.id));
    output.push_str("Model calls: performed only for planning when DEEPSEEK_API_KEY is set\n");
    output.push_str("Source file writes in worktrees: RAPTOR_PLAN.md only\n");
    output.push_str("Tests: not run\n");
    output.push_str("Merge: not performed\n");
    output.push_str("Remote push: not performed\n");
    output.push_str("Model output execution: not performed\n");
    output.push_str("Note: review and diff inspect swarm role worktrees read-only.\n");

    output
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

    output.push_str("Changed files:\n");
    push_changed_files_section(&mut output, &diff.changed_files);
    output.push_str("Unstaged diff:\n");
    push_diff_section(&mut output, &diff.unstaged);
    output.push_str("Staged diff:\n");
    push_diff_section(&mut output, &diff.staged);

    output
}

fn render_test_run(task_id: &str, mode: &str, results: &[TestTargetResult]) -> String {
    let passed = results
        .iter()
        .filter(|result| result.output.success)
        .count();
    let mut output = format!(
        "RaptorCode Test Run\nTask: {task_id}\nMode: {mode}\nTargets: {}\nSummary: {passed}/{} passed\n",
        results.len(),
        results.len()
    );

    for result in results {
        output.push_str(&format!(
            "\nTarget: {}\nWorktree: {}\nCommand: {}\nCommand source: {}\nStatus: {}\nExit code: {}\n",
            result.label,
            result.output.worktree_path.display(),
            result.output.command.display(),
            result.output.command.source,
            if result.output.success { "passed" } else { "failed" },
            display_exit_code(result.output.exit_code)
        ));

        output.push_str("Stdout:\n");
        push_command_output_section(&mut output, &result.output.stdout);
        output.push_str("Stderr:\n");
        push_command_output_section(&mut output, &result.output.stderr);
    }

    output.push_str("\nDependency installation: not performed by RaptorCode\n");
    output.push_str("Model calls: not performed\n");
    output.push_str("Model output execution: not performed\n");
    output.push_str("Source file writes by RaptorCode: not performed\n");
    output.push_str("Merge: not performed\n");
    output.push_str("Remote push: not performed\n");
    output
}

fn render_test_command_not_detected(task_id: &str, mode: &str, target: &TestTarget) -> String {
    format!(
        "RaptorCode Test Run\nTask: {task_id}\nMode: {mode}\nTarget: {}\nWorktree: {}\nStatus: blocked_no_test_command_detected\nDetection checked: Cargo.toml, package.json scripts.test, Makefile test target\nExplicit command example: raptor test {task_id} -- cargo test\nDependency installation: not performed by RaptorCode\nModel calls: not performed\nModel output execution: not performed\nSource file writes by RaptorCode: not performed\n",
        target.label,
        target.worktree_path.display()
    )
}

fn render_swarm_review(
    metadata: &SwarmMetadata,
    role: Option<&str>,
    reviews: &[SwarmRoleReview<'_>],
) -> String {
    let mut output = format!(
        "RaptorCode Swarm Review\nTask: {}\nDescription: {}\nStatus: {}\nPlanning status: {}\nRole filter: {}\nAgents reviewed: {}\n",
        metadata.id,
        metadata.description,
        metadata.status,
        metadata.planning_status.as_deref().unwrap_or("-"),
        role.unwrap_or("all"),
        reviews.len()
    );

    for item in reviews {
        output.push_str(&format!(
            "\nAgent: {}\nStatus: {}\nPlanning status: {}\nImplementation status: {}\nWorktree: {}\nPlan file: {}\nProposal file: {}\n",
            item.agent.role,
            item.agent.status,
            item.agent.planning_status.as_deref().unwrap_or("-"),
            item.agent.implementation_status.as_deref().unwrap_or("-"),
            item.review.worktree_path.display(),
            display_path_optional(item.agent.plan_file_path.as_ref()),
            display_path_optional(item.agent.proposal_file_path.as_ref())
        ));

        output.push_str("Git status:\n");
        if item.review.status_lines.is_empty() {
            output.push_str("  clean\n");
        } else {
            for line in &item.review.status_lines {
                output.push_str("  ");
                output.push_str(line);
                output.push('\n');
            }
        }

        output.push_str("Changed files:\n");
        push_changed_files_section(&mut output, &item.review.changed_files);
    }

    output.push_str(
        "\nRead-only: no files, metadata, git index, commits, merges, or pushes were modified.\n",
    );
    output
}

fn render_swarm_diff(
    metadata: &SwarmMetadata,
    role: Option<&str>,
    diffs: &[SwarmRoleDiff<'_>],
) -> String {
    let mut output = format!(
        "RaptorCode Swarm Diff\nTask: {}\nDescription: {}\nStatus: {}\nRole filter: {}\nAgents inspected: {}\n",
        metadata.id,
        metadata.description,
        metadata.status,
        role.unwrap_or("all"),
        diffs.len()
    );

    for item in diffs {
        output.push_str(&format!(
            "\nAgent: {}\nWorktree: {}\nProposal file: {}\n",
            item.agent.role,
            item.diff.worktree_path.display(),
            display_path_optional(item.agent.proposal_file_path.as_ref())
        ));
        output.push_str("Changed files:\n");
        push_changed_files_section(&mut output, &item.diff.changed_files);
        output.push_str("Unstaged diff:\n");
        push_diff_section(&mut output, &item.diff.unstaged);
        output.push_str("Staged diff:\n");
        push_diff_section(&mut output, &item.diff.staged);
    }

    output.push_str("\nUntracked files are listed under Changed files; git does not include their contents in standard staged or unstaged diff output until they are added.\n");
    output.push_str(
        "Read-only: no files, metadata, git index, commits, merges, or pushes were modified.\n",
    );
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

fn push_changed_files_section(output: &mut String, changed_files: &[String]) {
    if changed_files.is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for file in changed_files {
        output.push_str("  ");
        output.push_str(file);
        output.push('\n');
    }
}

fn push_command_output_section(output: &mut String, command_output: &str) {
    if command_output.trim().is_empty() {
        output.push_str("  (none)\n");
        return;
    }

    for line in command_output.lines() {
        output.push_str("  ");
        output.push_str(line);
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

fn display_number_optional(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_owned())
}

fn display_exit_code(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "terminated_by_signal".to_owned())
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
    use raptor_core::{SwarmAgentRole, TaskId};
    use raptor_providers::{ProviderCapabilities, ProviderError};
    use raptor_store::{read_swarm_metadata, read_task_metadata, CONFIG_FILE_NAME};
    use std::cell::Cell;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDir {
        path: PathBuf,
    }

    struct FakeDeepSeekProvider {
        capabilities: ProviderCapabilities,
        call_count: Cell<usize>,
        default_model: ModelId,
        fail_role: Option<String>,
        provider_id: ProviderId,
        response: CompletionResponse,
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

    impl FakeDeepSeekProvider {
        fn new(content: &str) -> Self {
            let provider_id = ProviderId::new("deepseek").unwrap();
            let default_model = ModelId::new("deepseek-chat").unwrap();

            Self {
                capabilities: ProviderCapabilities::network_chat(),
                call_count: Cell::new(0),
                default_model: default_model.clone(),
                fail_role: None,
                provider_id: provider_id.clone(),
                response: CompletionResponse {
                    provider_id,
                    model_id: default_model,
                    content: content.to_owned(),
                    finish_reason: Some("stop".to_owned()),
                    usage: Some(TokenUsage {
                        completion_tokens: Some(2),
                        prompt_tokens: Some(3),
                        total_tokens: Some(5),
                    }),
                },
            }
        }

        fn failing_for_role(content: &str, role: &str) -> Self {
            let mut provider = Self::new(content);
            provider.fail_role = Some(role.to_owned());
            provider
        }

        fn call_count(&self) -> usize {
            self.call_count.get()
        }
    }

    impl Provider for FakeDeepSeekProvider {
        fn id(&self) -> &ProviderId {
            &self.provider_id
        }

        fn display_name(&self) -> &str {
            "DeepSeek"
        }

        fn capabilities(&self) -> &ProviderCapabilities {
            &self.capabilities
        }

        fn default_model(&self) -> Option<&ModelId> {
            Some(&self.default_model)
        }

        fn models(&self) -> &[ModelId] {
            std::slice::from_ref(&self.default_model)
        }

        fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, ProviderError> {
            self.call_count.set(self.call_count.get() + 1);
            assert_eq!(request.provider_id.as_str(), "deepseek");
            assert_eq!(request.model_id.as_str(), "deepseek-chat");
            assert_eq!(request.messages.len(), 2);
            assert_eq!(request.messages[0].role.as_str(), "system");
            assert_eq!(request.messages[1].role.as_str(), "user");
            if let Some(fail_role) = self.fail_role.as_deref() {
                if request.messages[1].content.contains(fail_role) {
                    return Err(ProviderError::RateLimited("test rate limit".to_owned()));
                }
            }
            Ok(self.response.clone())
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

    fn commit_cargo_fixture(path: &Path) {
        fs::create_dir_all(path.join("src")).unwrap();
        fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"raptor_test_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
        )
        .unwrap();
        fs::write(
            path.join("src/lib.rs"),
            "pub fn add(left: i32, right: i32) -> i32 { left + right }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn adds_numbers() {\n        assert_eq!(add(2, 2), 4);\n    }\n}\n",
        )
        .unwrap();
        run_git(path, &["add", "Cargo.toml", "src/lib.rs"]);
        run_git(
            path,
            &[
                "-c",
                "user.name=Raptor Test",
                "-c",
                "user.email=raptor@example.invalid",
                "commit",
                "-m",
                "add cargo fixture",
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

    fn swarm_metadata_path(repository_root: &Path, task_id: &TaskId) -> PathBuf {
        repository_root
            .join(".raptor")
            .join("swarms")
            .join(format!("{}.toml", task_id.as_str()))
    }

    fn swarm_agent_worktree_path(repository_root: &Path, task_id: &TaskId, role: &str) -> PathBuf {
        repository_root
            .join(".raptor")
            .join("worktrees")
            .join(task_id.as_str())
            .join(role)
    }

    fn agent_metadata_path(repository_root: &Path, agent_id: &AgentId) -> PathBuf {
        repository_root
            .join(".raptor")
            .join("agents")
            .join(format!("{}.toml", agent_id.as_str()))
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

    fn replace_swarm_worktree_path_in_metadata(
        repository_root: &Path,
        task_id: &TaskId,
        role: &str,
        replacement_path: &Path,
    ) {
        let metadata_path = swarm_metadata_path(repository_root, task_id);
        let worktree_path = swarm_agent_worktree_path(repository_root, task_id, role);
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
        let commands = [("init", CliCommand::Init), ("doctor", CliCommand::Doctor)];

        for (arg, expected) in commands {
            assert_eq!(parse_args([arg]).unwrap(), expected);
        }
    }

    #[test]
    fn parses_chat_message() {
        assert_eq!(
            parse_args(["chat", "hello", "deepseek"]).unwrap(),
            CliCommand::Chat {
                message: "hello deepseek".to_owned()
            }
        );
    }

    #[test]
    fn parses_swarm_description() {
        assert_eq!(
            parse_args(["swarm", "add", "oauth"]).unwrap(),
            CliCommand::Swarm {
                description: "add oauth".to_owned()
            }
        );
    }

    #[test]
    fn parses_swarm_implement_command() {
        assert_eq!(
            parse_args([
                "swarm",
                "implement",
                "add-github-oauth-123",
                "frontend-agent"
            ])
            .unwrap(),
            CliCommand::SwarmImplement {
                task_id: "add-github-oauth-123".to_owned(),
                role: "frontend-agent".to_owned()
            }
        );
    }

    #[test]
    fn parses_provider_and_agent_inspection_commands() {
        assert_eq!(
            parse_args(["provider", "list"]).unwrap(),
            CliCommand::Provider(ProviderCommand::List)
        );
        assert_eq!(
            parse_args(["agent", "list"]).unwrap(),
            CliCommand::Agent(AgentCommand::List)
        );
        assert_eq!(
            parse_args(["agent", "show", "agent-1"]).unwrap(),
            CliCommand::Agent(AgentCommand::Show {
                id: "agent-1".to_owned()
            })
        );
        assert_eq!(
            parse_args(["agent", "policy"]).unwrap(),
            CliCommand::Agent(AgentCommand::Policy)
        );
        assert_eq!(
            parse_args([
                "agent",
                "create",
                "--role",
                "worker",
                "--task",
                "task-1",
                "--provider",
                "deepseek",
                "--model",
                "deepseek-chat"
            ])
            .unwrap(),
            CliCommand::Agent(AgentCommand::Create {
                model_id: Some("deepseek-chat".to_owned()),
                provider_id: Some("deepseek".to_owned()),
                role: "worker".to_owned(),
                task_id: Some("task-1".to_owned())
            })
        );
    }

    #[test]
    fn parses_review_and_diff_task_ids() {
        assert_eq!(
            parse_args(["review", "task-123"]).unwrap(),
            CliCommand::Review {
                role: None,
                task_id: "task-123".to_owned()
            }
        );
        assert_eq!(
            parse_args(["diff", "task-123"]).unwrap(),
            CliCommand::Diff {
                role: None,
                task_id: "task-123".to_owned()
            }
        );
        assert_eq!(
            parse_args(["review", "task-123", "frontend-agent"]).unwrap(),
            CliCommand::Review {
                role: Some("frontend-agent".to_owned()),
                task_id: "task-123".to_owned()
            }
        );
        assert_eq!(
            parse_args(["diff", "task-123", "frontend-agent"]).unwrap(),
            CliCommand::Diff {
                role: Some("frontend-agent".to_owned()),
                task_id: "task-123".to_owned()
            }
        );
    }

    #[test]
    fn parses_test_task_id_and_explicit_command() {
        assert_eq!(
            parse_args(["test", "task-123"]).unwrap(),
            CliCommand::Test {
                task_id: "task-123".to_owned(),
                test_command: None
            }
        );
        assert_eq!(
            parse_args(["test", "task-123", "--", "cargo", "test", "--quiet"]).unwrap(),
            CliCommand::Test {
                task_id: "task-123".to_owned(),
                test_command: Some(vec![
                    "cargo".to_owned(),
                    "test".to_owned(),
                    "--quiet".to_owned()
                ])
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
    fn rejects_missing_chat_message() {
        let error = parse_args(["chat"]).unwrap_err();

        assert!(error.to_string().contains("chat message"));
    }

    #[test]
    fn rejects_missing_swarm_description() {
        let error = parse_args(["swarm"]).unwrap_err();

        assert!(error.to_string().contains("description"));
    }

    #[test]
    fn rejects_missing_swarm_implement_arguments() {
        let missing_task = parse_args(["swarm", "implement"]).unwrap_err();
        let missing_role = parse_args(["swarm", "implement", "task-123"]).unwrap_err();
        let extra =
            parse_args(["swarm", "implement", "task-123", "frontend-agent", "extra"]).unwrap_err();

        assert!(missing_task.to_string().contains("task id"));
        assert!(missing_role.to_string().contains("role"));
        assert!(extra
            .to_string()
            .contains("exactly one task id and one role"));
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
    fn rejects_missing_and_extra_test_arguments() {
        let missing_error = parse_args(["test"]).unwrap_err();
        let empty_explicit = parse_args(["test", "task-123", "--"]).unwrap_err();
        let extra_error = parse_args(["test", "task-123", "cargo", "test"]).unwrap_err();

        assert!(missing_error.to_string().contains("task id"));
        assert!(empty_explicit.to_string().contains("required after --"));
        assert!(extra_error
            .to_string()
            .contains("optional explicit command"));
    }

    #[test]
    fn rejects_extra_review_and_diff_arguments() {
        let review_error =
            parse_args(["review", "task-123", "frontend-agent", "extra"]).unwrap_err();
        let diff_error = parse_args(["diff", "task-123", "frontend-agent", "extra"]).unwrap_err();

        assert!(review_error.to_string().contains("optional swarm role"));
        assert!(diff_error.to_string().contains("optional swarm role"));
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
    fn rejects_missing_provider_and_agent_subcommands() {
        let provider_error = parse_args(["provider"]).unwrap_err();
        let agent_error = parse_args(["agent"]).unwrap_err();
        let agent_show_error = parse_args(["agent", "show"]).unwrap_err();
        let agent_create_error = parse_args(["agent", "create"]).unwrap_err();

        assert!(provider_error.to_string().contains("provider command"));
        assert!(agent_error.to_string().contains("agent command"));
        assert!(agent_show_error.to_string().contains("agent id"));
        assert!(agent_create_error.to_string().contains("--role"));
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
    fn provider_list_handles_missing_config_without_writing_files() {
        let test_dir = TestDir::create("provider-list-missing");
        let config_path = test_dir.path.join(CONFIG_FILE_NAME);

        let output = run(["provider", "list"], &test_dir.path).unwrap();

        assert!(output.contains("Provider configuration: not configured"));
        assert!(output.contains("deepseek"));
        assert!(output.contains("not configured"));
        assert!(output.contains("Execution: disabled"));
        assert!(!config_path.exists());
    }

    #[test]
    fn provider_list_reads_deepseek_config() {
        let test_dir = TestDir::create("provider-list-configured");

        run(["init"], &test_dir.path).unwrap();
        let output = run(["provider", "list"], &test_dir.path).unwrap();

        assert!(output.contains("Provider configuration: configured"));
        assert!(output.contains("deepseek"));
        assert!(output.contains("enabled"));
        assert!(output.contains("deepseek-chat"));
        assert!(output.contains("network-required"));
    }

    #[test]
    fn chat_rejects_empty_message() {
        let provider = FakeDeepSeekProvider::new("unused");
        let error = chat_with_provider("   ", &provider).unwrap_err();

        assert!(error.to_string().contains("chat message"));
    }

    #[test]
    fn chat_renders_deepseek_completion_without_side_effects() {
        let test_dir = TestDir::create("chat-no-side-effects");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let before_head = git_text(&test_dir.path, &["rev-parse", "HEAD"]);
        let provider = FakeDeepSeekProvider::new("Use OAuth callback validation.");

        let output = chat_with_provider("How should I add OAuth?", &provider).unwrap();

        assert!(output.contains("DeepSeek response"));
        assert!(output.contains("Provider: deepseek"));
        assert!(output.contains("Model: deepseek-chat"));
        assert!(output.contains("Use OAuth callback validation."));
        assert!(output.contains("Model output execution: not performed"));
        assert!(output.contains("Source file writes: not performed"));
        assert!(output.contains("Shell execution: not performed"));
        assert_eq!(
            fs::read_to_string(test_dir.path.join("tracked.txt")).unwrap(),
            "source\n"
        );
        assert_eq!(
            git_text(&test_dir.path, &["rev-parse", "HEAD"]),
            before_head
        );
        assert_eq!(
            git_text(
                &test_dir.path,
                &["status", "--short", "--untracked-files=no"]
            ),
            ""
        );
    }

    #[test]
    fn agent_list_handles_empty_local_state() {
        let test_dir = TestDir::create("agent-list-empty");
        initialize_git_repo(&test_dir.path);

        let output = run(["agent", "list"], &test_dir.path).unwrap();

        assert_eq!(
            output,
            "No RaptorCode agent sessions found.\nExecution: read-only foundation only; no model calls, shell execution, or file modification are configured.\n"
        );
    }

    #[test]
    fn agent_show_validates_id_and_reports_missing_session() {
        let test_dir = TestDir::create("agent-show-missing");
        initialize_git_repo(&test_dir.path);

        let unsafe_error = run(["agent", "show", "../agent"], &test_dir.path).unwrap_err();
        let missing_error = run(["agent", "show", "agent-1"], &test_dir.path).unwrap_err();

        assert!(unsafe_error.to_string().contains("not safe"));
        assert!(missing_error.to_string().contains("not found"));
    }

    #[test]
    fn agent_create_persists_metadata_only() {
        let test_dir = TestDir::create("agent-create");
        initialize_git_repo(&test_dir.path);
        let agent_id = AgentId::from_session_parts(
            AgentRole::Worker,
            None,
            Some("deepseek"),
            Some("deepseek-chat"),
        )
        .unwrap();

        let output = run(
            [
                "agent",
                "create",
                "--role",
                "worker",
                "--provider",
                "deepseek",
                "--model",
                "deepseek-chat",
            ],
            &test_dir.path,
        )
        .unwrap();
        let metadata_path = agent_metadata_path(&test_dir.path, &agent_id);
        let metadata = fs::read_to_string(metadata_path).unwrap();

        assert!(output.contains("Agent session created"));
        assert!(output.contains(agent_id.as_str()));
        assert!(output.contains("Model execution: not performed"));
        assert!(output.contains("Shell execution: not performed"));
        assert!(output.contains("Source file writes: not performed"));
        assert!(metadata.contains(&format!("id = \"{}\"", agent_id.as_str())));
        assert!(metadata.contains("role = \"worker\""));
        assert!(metadata.contains("execution_mode = \"read_only\""));
        assert!(metadata.contains("provider_id = \"deepseek\""));
        assert!(metadata.contains("model_id = \"deepseek-chat\""));
    }

    #[test]
    fn agent_create_rejects_duplicate_deterministic_session() {
        let test_dir = TestDir::create("agent-create-duplicate");
        initialize_git_repo(&test_dir.path);

        run(["agent", "create", "--role", "worker"], &test_dir.path).unwrap();
        let error = run(["agent", "create", "--role", "worker"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn agent_list_reads_persisted_session() {
        let test_dir = TestDir::create("agent-list-one");
        initialize_git_repo(&test_dir.path);
        let agent_id = AgentId::from_session_parts(AgentRole::Reviewer, None, None, None).unwrap();

        run(["agent", "create", "--role", "reviewer"], &test_dir.path).unwrap();
        let output = run(["agent", "list"], &test_dir.path).unwrap();

        assert!(output.contains("ID | Role | State | Execution mode"));
        assert!(output.contains(agent_id.as_str()));
        assert!(output.contains("reviewer"));
        assert!(output.contains("read_only"));
    }

    #[test]
    fn agent_show_reads_persisted_session() {
        let test_dir = TestDir::create("agent-show-existing");
        initialize_git_repo(&test_dir.path);
        let agent_id = AgentId::from_session_parts(
            AgentRole::Architect,
            None,
            Some("deepseek"),
            Some("deepseek-chat"),
        )
        .unwrap();

        run(
            [
                "agent",
                "create",
                "--role",
                "architect",
                "--provider",
                "deepseek",
                "--model",
                "deepseek-chat",
            ],
            &test_dir.path,
        )
        .unwrap();
        let output = run(["agent", "show", agent_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains(&format!("Agent: {}", agent_id.as_str())));
        assert!(output.contains("Role: architect"));
        assert!(output.contains("Provider: deepseek"));
        assert!(output.contains("Model: deepseek-chat"));
        assert!(output.contains("Metadata:"));
    }

    #[test]
    fn agent_create_rejects_unsafe_bindings_and_ids() {
        let test_dir = TestDir::create("agent-create-unsafe");
        initialize_git_repo(&test_dir.path);

        let task_error = run(
            ["agent", "create", "--role", "worker", "--task", "../task"],
            &test_dir.path,
        )
        .unwrap_err();
        let provider_error = run(
            ["agent", "create", "--role", "worker", "--provider", "Bad"],
            &test_dir.path,
        )
        .unwrap_err();
        let model_error = run(
            ["agent", "create", "--role", "worker", "--model", "../model"],
            &test_dir.path,
        )
        .unwrap_err();

        assert!(task_error.to_string().contains("not safe"));
        assert!(provider_error.to_string().contains("not safe"));
        assert!(model_error.to_string().contains("not safe"));
    }

    #[test]
    fn agent_create_binds_existing_task_metadata() {
        let test_dir = TestDir::create("agent-create-task");
        initialize_git_repo(&test_dir.path);
        let description = "Bind agent task";
        let task_id = TaskId::from_description(description).unwrap();
        let agent_id =
            AgentId::from_session_parts(AgentRole::Worker, Some(task_id.as_str()), None, None)
                .unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let output = run(
            [
                "agent",
                "create",
                "--role",
                "worker",
                "--task",
                task_id.as_str(),
            ],
            &test_dir.path,
        )
        .unwrap();

        assert!(output.contains(task_id.as_str()));
        let stored = run(["agent", "show", agent_id.as_str()], &test_dir.path).unwrap();
        assert!(stored.contains(&format!("Task: {}", task_id.as_str())));
        assert!(stored.contains("Worktree:"));
    }

    #[test]
    fn agent_policy_displays_deny_by_default_rules() {
        let output = run(["agent", "policy"], Path::new(".")).unwrap();

        assert!(output.contains("Approval mode: manual"));
        assert!(output.contains("File read: repository_root_only"));
        assert!(output.contains("File write: deny"));
        assert!(output.contains("Shell execution: deny"));
        assert!(output.contains("Network: deny"));
        assert!(output.contains("Model execution: deny_until_configured_and_explicit"));
        assert!(output.contains("File write allowed: no"));
        assert!(output.contains("Shell execution allowed: no"));
        assert!(output.contains("Network allowed: no"));
        assert!(output.contains("Model execution allowed: no"));
    }

    #[test]
    fn agent_create_does_not_modify_source_files() {
        let test_dir = TestDir::create("agent-create-no-source-effects");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");

        run(["agent", "create", "--role", "worker"], &test_dir.path).unwrap();

        assert_eq!(
            fs::read_to_string(test_dir.path.join("tracked.txt")).unwrap(),
            "source\n"
        );
        assert_eq!(
            git_text(
                &test_dir.path,
                &["status", "--short", "--untracked-files=no"]
            ),
            ""
        );
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
    fn swarm_creates_four_role_worktrees_and_metadata() {
        let test_dir = TestDir::create("swarm");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Add GitHub OAuth to this Next.js app";
        let task_id = TaskId::from_description(description).unwrap();

        let output = create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::MissingDeepSeekApiKey,
        )
        .unwrap();
        let metadata_path = swarm_metadata_path(&test_dir.path, &task_id);
        let metadata = fs::read_to_string(&metadata_path).unwrap();
        let roles = [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ];

        assert!(output.contains("RaptorCode Swarm Created"));
        assert!(output.contains(task_id.as_str()));
        assert!(output.contains("Provider: DeepSeek"));
        assert!(output.contains("Status: blocked_missing_deepseek_api_key"));
        assert!(output.contains(DEEPSEEK_API_KEY_URL));
        assert!(output.contains(&format!("raptor review {}", task_id.as_str())));
        assert!(output.contains(&format!("raptor diff {}", task_id.as_str())));
        assert!(output.contains("Source file writes in worktrees: RAPTOR_PLAN.md only"));
        assert!(metadata_path.exists());
        assert!(metadata.contains(&format!("id = \"{}\"", task_id.as_str())));
        assert!(metadata.contains("provider = \"deepseek\""));
        assert!(metadata.contains("model = \"deepseek-chat\""));
        assert!(metadata.contains("status = \"blocked_missing_deepseek_api_key\""));
        assert!(metadata.contains("planning_status = \"blocked_missing_deepseek_api_key\""));

        for role in roles {
            let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, role);
            let branch_name = format!("raptor/{}/{role}", task_id.as_str());
            let branch_ref = format!("refs/heads/{branch_name}");

            assert!(worktree_path.is_dir());
            assert!(worktree_path.starts_with(test_dir.path.join(".raptor").join("worktrees")));
            assert!(output.contains(role));
            assert!(output.contains(&branch_name));
            assert!(metadata.contains(&format!("role = \"{role}\"")));
            assert!(metadata.contains(&format!("agent_id = \"{}-{role}\"", task_id.as_str())));
            assert!(metadata.contains(&format!("branch = \"{branch_name}\"")));
            assert!(!worktree_path.join("RAPTOR_PLAN.md").exists());
            run_git(
                &test_dir.path,
                &["show-ref", "--verify", "--quiet", &branch_ref],
            );
            assert_eq!(
                git_text(&worktree_path, &["status", "--short"]),
                "",
                "{role} worktree should be clean"
            );
        }

        assert_eq!(
            fs::read_to_string(test_dir.path.join("tracked.txt")).unwrap(),
            "source\n"
        );
        assert_eq!(
            git_text(
                &test_dir.path,
                &["status", "--short", "--untracked-files=no"]
            ),
            ""
        );
    }

    #[test]
    fn swarm_deepseek_planning_writes_role_plans_and_metadata() {
        let test_dir = TestDir::create("swarm-planning");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(
            &test_dir.path,
            "src/app.tsx",
            "export default function App() {}\n",
        );
        let description = "Add GitHub OAuth to this Next.js app";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new(
            "1. Inspect relevant files.\n2. Plan implementation only.\n3. Do not execute output.",
        );

        let output = create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        let metadata = fs::read_to_string(swarm_metadata_path(&test_dir.path, &task_id)).unwrap();

        assert_eq!(provider.call_count(), 4);
        assert!(output.contains("Planning status: planning_completed"));
        assert!(metadata.contains("status = \"planning_completed\""));
        assert!(metadata.contains("planning_status = \"planning_completed\""));
        assert_eq!(
            fs::read_to_string(test_dir.path.join("src/app.tsx")).unwrap(),
            "export default function App() {}\n"
        );

        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, role);
            let plan_path = worktree_path.join(RAPTOR_PLAN_FILE_NAME);
            let plan = fs::read_to_string(&plan_path).unwrap();

            assert!(plan_path.starts_with(&worktree_path));
            assert!(plan.contains("# RaptorCode Agent Plan"));
            assert!(plan.contains(&format!("Task id: {}", task_id.as_str())));
            assert!(plan.contains(&format!("Agent role: {role}")));
            assert!(plan.contains("Provider: DeepSeek"));
            assert!(plan.contains("Model: deepseek-chat"));
            assert!(plan.contains("This plan is AI-generated"));
            assert!(metadata.contains(&format!("role = \"{role}\"")));
            assert!(metadata.contains(&format!("plan_file_path = \"{}\"", plan_path.display())));
            assert_eq!(
                git_text(&worktree_path, &["status", "--short"]),
                "?? RAPTOR_PLAN.md\n"
            );
        }
    }

    #[test]
    fn swarm_missing_api_key_blocks_planning_without_destroying_worktrees() {
        let test_dir = TestDir::create("swarm-missing-key");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Plan with missing key";
        let task_id = TaskId::from_description(description).unwrap();

        let output = create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::MissingDeepSeekApiKey,
        )
        .unwrap();
        let metadata = fs::read_to_string(swarm_metadata_path(&test_dir.path, &task_id)).unwrap();

        assert!(output.contains("blocked_missing_deepseek_api_key"));
        assert!(output.contains(DEEPSEEK_API_KEY_URL));
        assert!(metadata.contains("planning_status = \"blocked_missing_deepseek_api_key\""));

        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, role);

            assert!(worktree_path.is_dir());
            assert!(!worktree_path.join(RAPTOR_PLAN_FILE_NAME).exists());
        }
    }

    #[test]
    fn swarm_model_output_is_not_executed_or_applied_to_source() {
        let test_dir = TestDir::create("swarm-output-safety");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(
            &test_dir.path,
            "src/app.tsx",
            "export default function App() {}\n",
        );
        let description = "Plan source safety";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new(
            "Run `touch SHOULD_NOT_EXIST` and replace src/app.tsx. This must stay text only.",
        );

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();

        assert!(!test_dir.path.join("SHOULD_NOT_EXIST").exists());
        assert_eq!(
            fs::read_to_string(test_dir.path.join("src/app.tsx")).unwrap(),
            "export default function App() {}\n"
        );

        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, role);
            let source = fs::read_to_string(worktree_path.join("src/app.tsx")).unwrap();

            assert_eq!(source, "export default function App() {}\n");
            assert!(worktree_path.join(RAPTOR_PLAN_FILE_NAME).exists());
            assert!(!worktree_path.join("SHOULD_NOT_EXIST").exists());
        }
    }

    #[test]
    fn swarm_records_per_agent_failure_without_losing_metadata() {
        let test_dir = TestDir::create("swarm-agent-failure");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Plan with one failed role";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::failing_for_role("plan", "security-agent");

        let output = create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        let metadata_path = swarm_metadata_path(&test_dir.path, &task_id);
        let metadata = fs::read_to_string(metadata_path).unwrap();

        assert_eq!(provider.call_count(), 4);
        assert!(output.contains("Planning status: planning_failed"));
        assert!(metadata.contains("planning_status = \"planning_failed\""));
        assert!(metadata.contains("role = \"security-agent\""));

        for role in ["frontend-agent", "backend-agent", "test-agent"] {
            assert!(swarm_agent_worktree_path(&test_dir.path, &task_id, role)
                .join(RAPTOR_PLAN_FILE_NAME)
                .exists());
        }
        assert!(
            !swarm_agent_worktree_path(&test_dir.path, &task_id, "security-agent")
                .join(RAPTOR_PLAN_FILE_NAME)
                .exists()
        );
    }

    #[test]
    fn swarm_implement_generates_supervised_proposal_for_selected_role() {
        let test_dir = TestDir::create("swarm-implement");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(
            &test_dir.path,
            "src/app.tsx",
            "export default function App() {}\n",
        );
        let description = "Add GitHub OAuth to this Next.js app";
        let task_id = TaskId::from_description(description).unwrap();
        let planning_provider = FakeDeepSeekProvider::new("Inspect src/app.tsx before changes.");
        let implementation_provider = FakeDeepSeekProvider::new(
            "Proposal: update OAuth UI manually. Patch-like content stays in this file.",
        );

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&planning_provider),
        )
        .unwrap();
        let output = implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "frontend-agent",
            SwarmImplementationSource::Provider(&implementation_provider),
        )
        .unwrap();
        let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, "frontend-agent");
        let proposal_path = worktree_path.join(RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME);
        let proposal = fs::read_to_string(&proposal_path).unwrap();
        let metadata = read_swarm_metadata(&test_dir.path, task_id.as_str()).unwrap();
        let agent = metadata
            .agents
            .iter()
            .find(|agent| agent.role == "frontend-agent")
            .unwrap();

        assert_eq!(implementation_provider.call_count(), 1);
        assert!(output.contains("RaptorCode Implementation Proposal Generated"));
        assert!(output.contains(&proposal_path.display().to_string()));
        assert!(proposal_path.starts_with(&worktree_path));
        assert!(proposal.contains("# RaptorCode Implementation Proposal"));
        assert!(proposal.contains(&format!("Task id: {}", task_id.as_str())));
        assert!(proposal.contains("Role: frontend-agent"));
        assert!(proposal.contains("Provider: DeepSeek"));
        assert!(proposal.contains("Model: deepseek-chat"));
        assert!(proposal.contains("Patch-like content stays in this file"));
        assert!(proposal.contains("RaptorCode did not execute model output"));
        assert_eq!(
            agent.implementation_status.as_deref(),
            Some(SWARM_PROPOSAL_GENERATED_STATUS)
        );
        assert_eq!(agent.proposal_file_path.as_ref(), Some(&proposal_path));
        assert_eq!(
            fs::read_to_string(test_dir.path.join("src/app.tsx")).unwrap(),
            "export default function App() {}\n"
        );
    }

    #[test]
    fn swarm_implement_missing_api_key_does_not_write_proposal() {
        let test_dir = TestDir::create("swarm-implement-missing-key");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Missing key implementation";
        let task_id = TaskId::from_description(description).unwrap();
        let planning_provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&planning_provider),
        )
        .unwrap();
        let output = implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "backend-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap();
        let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, "backend-agent");
        let metadata = read_swarm_metadata(&test_dir.path, task_id.as_str()).unwrap();
        let agent = metadata
            .agents
            .iter()
            .find(|agent| agent.role == "backend-agent")
            .unwrap();

        assert!(output.contains(DEEPSEEK_API_KEY_URL));
        assert!(!worktree_path
            .join(RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME)
            .exists());
        assert_eq!(
            agent.implementation_status.as_deref(),
            Some(SWARM_BLOCKED_MISSING_DEEPSEEK_API_KEY_STATUS)
        );
        assert!(agent.proposal_file_path.is_none());
    }

    #[test]
    fn swarm_implement_rejects_unsafe_task_ids_roles_and_missing_metadata() {
        let test_dir = TestDir::create("swarm-implement-validation");
        initialize_git_repo(&test_dir.path);

        let unsafe_error = implement_swarm_role_with_source(
            &test_dir.path,
            "../secret",
            "frontend-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();
        let traversal_error = implement_swarm_role_with_source(
            &test_dir.path,
            "bad..task",
            "frontend-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();
        let role_error = implement_swarm_role_with_source(
            &test_dir.path,
            "missing-task",
            "../frontend-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();
        let missing_error = implement_swarm_role_with_source(
            &test_dir.path,
            "missing-task",
            "frontend-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();

        assert!(unsafe_error.to_string().contains("swarm id"));
        assert!(traversal_error.to_string().contains("swarm id"));
        assert!(role_error.to_string().contains("unknown swarm agent role"));
        assert!(missing_error
            .to_string()
            .contains("swarm metadata not found"));
    }

    #[test]
    fn swarm_implement_rejects_missing_role_and_worktree() {
        let test_dir = TestDir::create("swarm-implement-missing-worktree");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Missing role worktree";
        let task_id = TaskId::from_description(description).unwrap();
        let planning_provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&planning_provider),
        )
        .unwrap();
        let missing_role = implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "docs-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();

        fs::remove_dir_all(swarm_agent_worktree_path(
            &test_dir.path,
            &task_id,
            "test-agent",
        ))
        .unwrap();

        let missing_worktree = implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "test-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();

        assert!(missing_role
            .to_string()
            .contains("unknown swarm agent role"));
        assert!(missing_worktree.to_string().contains("worktree not found"));
    }

    #[test]
    fn swarm_implement_rejects_worktree_path_outside_assigned_role() {
        let test_dir = TestDir::create("swarm-implement-unsafe-path");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Unsafe proposal path";
        let task_id = TaskId::from_description(description).unwrap();
        let planning_provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&planning_provider),
        )
        .unwrap();
        replace_swarm_worktree_path_in_metadata(
            &test_dir.path,
            &task_id,
            "security-agent",
            &test_dir.path.join("outside-security-agent"),
        );

        let error = implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "security-agent",
            SwarmImplementationSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();

        assert!(error.to_string().contains("safe project path"));
        assert!(!test_dir
            .path
            .join("outside-security-agent")
            .join(RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME)
            .exists());
    }

    #[test]
    fn swarm_implement_model_output_is_not_executed_or_applied_to_source() {
        let test_dir = TestDir::create("swarm-implement-output-safety");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(
            &test_dir.path,
            "src/app.tsx",
            "export default function App() {}\n",
        );
        let description = "Implementation output safety";
        let task_id = TaskId::from_description(description).unwrap();
        let planning_provider = FakeDeepSeekProvider::new("Plan source safety.");
        let implementation_provider = FakeDeepSeekProvider::new(
            "Run `touch SHOULD_NOT_EXIST` and replace src/app.tsx. Keep this as text only.",
        );

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&planning_provider),
        )
        .unwrap();
        implement_swarm_role_with_source(
            &test_dir.path,
            task_id.as_str(),
            "frontend-agent",
            SwarmImplementationSource::Provider(&implementation_provider),
        )
        .unwrap();

        let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, "frontend-agent");

        assert!(!test_dir.path.join("SHOULD_NOT_EXIST").exists());
        assert!(!worktree_path.join("SHOULD_NOT_EXIST").exists());
        assert_eq!(
            fs::read_to_string(test_dir.path.join("src/app.tsx")).unwrap(),
            "export default function App() {}\n"
        );
        assert_eq!(
            fs::read_to_string(worktree_path.join("src/app.tsx")).unwrap(),
            "export default function App() {}\n"
        );
        assert!(worktree_path
            .join(RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME)
            .exists());
    }

    #[test]
    fn swarm_rejects_empty_description() {
        let test_dir = TestDir::create("swarm-empty");
        initialize_git_repo(&test_dir.path);

        let error = run(["swarm", "   "], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("description is required"));
    }

    #[test]
    fn swarm_rejects_non_git_directory() {
        let test_dir = TestDir::create("swarm-non-git");

        let error = run(["swarm", "Add feature"], &test_dir.path).unwrap_err();

        assert!(error.to_string().contains("not inside a git repository"));
    }

    #[test]
    fn swarm_description_cannot_escape_worktree_directory() {
        let test_dir = TestDir::create("swarm-unsafe-description");
        initialize_git_repo(&test_dir.path);
        let description = "../../escape branch && rm -rf app";
        let task_id = TaskId::from_description(description).unwrap();

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::MissingDeepSeekApiKey,
        )
        .unwrap();

        assert!(!task_id.as_str().contains(".."));
        assert!(task_id
            .as_str()
            .chars()
            .all(|character| character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '-'));

        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let worktree_path = swarm_agent_worktree_path(&test_dir.path, &task_id, role);

            assert!(worktree_path.is_dir());
            assert!(worktree_path.starts_with(test_dir.path.join(".raptor").join("worktrees")));
            assert!(!worktree_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir)));
        }
    }

    #[test]
    fn swarm_uses_deterministic_safe_branch_names() {
        let task_id = TaskId::from_description("Deterministic swarm").unwrap();

        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let branch = format!("raptor/{}/{role}", task_id.as_str());

            assert!(branch.starts_with("raptor/"));
            assert!(!branch.contains(".."));
            assert!(branch.split('/').all(|segment| {
                !segment.is_empty()
                    && segment.chars().all(|character| {
                        character.is_ascii_lowercase()
                            || character.is_ascii_digit()
                            || character == '-'
                    })
            }));
        }
    }

    #[test]
    fn swarm_refuses_duplicate_metadata_or_worktrees() {
        let test_dir = TestDir::create("swarm-duplicate");
        initialize_git_repo(&test_dir.path);
        let description = "Duplicate swarm";

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::MissingDeepSeekApiKey,
        )
        .unwrap();
        let error = create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::MissingDeepSeekApiKey,
        )
        .unwrap_err();

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
    fn review_and_diff_swarm_all_role_worktrees() {
        let test_dir = TestDir::create("swarm-review-all");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Review every swarm role";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan artifact.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();

        let review_output = run(["review", task_id.as_str()], &test_dir.path).unwrap();
        let diff_output = run(["diff", task_id.as_str()], &test_dir.path).unwrap();

        assert!(review_output.contains("RaptorCode Swarm Review"));
        assert!(review_output.contains("Role filter: all"));
        assert!(review_output.contains("Agents reviewed: 4"));
        assert!(review_output.contains("Agent: frontend-agent"));
        assert!(review_output.contains("Agent: backend-agent"));
        assert!(review_output.contains("Agent: test-agent"));
        assert!(review_output.contains("Agent: security-agent"));
        assert!(review_output.contains("RAPTOR_PLAN.md"));
        assert!(review_output.contains("Read-only: no files"));
        assert!(diff_output.contains("RaptorCode Swarm Diff"));
        assert!(diff_output.contains("Agents inspected: 4"));
        assert!(diff_output.contains("RAPTOR_PLAN.md"));
        assert!(diff_output.contains("Untracked files are listed"));
        assert!(diff_output.contains("Read-only: no files"));
    }

    #[test]
    fn review_and_diff_swarm_selected_role_only() {
        let test_dir = TestDir::create("swarm-review-role");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(&test_dir.path, "src/app.tsx", "before\n");
        let description = "Review selected swarm role";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan selected role.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        let frontend_worktree =
            swarm_agent_worktree_path(&test_dir.path, &task_id, "frontend-agent");
        fs::write(frontend_worktree.join("src/app.tsx"), "after\n").unwrap();

        let review_output = run(
            ["review", task_id.as_str(), "frontend-agent"],
            &test_dir.path,
        )
        .unwrap();
        let diff_output =
            run(["diff", task_id.as_str(), "frontend-agent"], &test_dir.path).unwrap();

        assert!(review_output.contains("Role filter: frontend-agent"));
        assert!(review_output.contains("Agents reviewed: 1"));
        assert!(review_output.contains("Agent: frontend-agent"));
        assert!(!review_output.contains("Agent: backend-agent"));
        assert!(review_output.contains("M src/app.tsx"));
        assert!(diff_output.contains("Role filter: frontend-agent"));
        assert!(diff_output.contains("Agents inspected: 1"));
        assert!(diff_output.contains("Agent: frontend-agent"));
        assert!(!diff_output.contains("Agent: backend-agent"));
        assert!(diff_output.contains("diff --git"));
        assert!(diff_output.contains("-before"));
        assert!(diff_output.contains("+after"));
    }

    #[test]
    fn swarm_review_and_diff_reject_invalid_role_missing_metadata_and_worktree() {
        let test_dir = TestDir::create("swarm-review-rejects");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let missing_review = run(["review", "missing-task"], &test_dir.path).unwrap_err();
        let missing_diff = run(["diff", "missing-task"], &test_dir.path).unwrap_err();
        let invalid_missing_review =
            run(["review", "missing-task", "docs-agent"], &test_dir.path).unwrap_err();
        let invalid_missing_diff =
            run(["diff", "missing-task", "docs-agent"], &test_dir.path).unwrap_err();
        let description = "Reject unsafe swarm review";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();

        let invalid_review =
            run(["review", task_id.as_str(), "docs-agent"], &test_dir.path).unwrap_err();
        let invalid_diff =
            run(["diff", task_id.as_str(), "docs-agent"], &test_dir.path).unwrap_err();

        fs::remove_dir_all(swarm_agent_worktree_path(
            &test_dir.path,
            &task_id,
            "test-agent",
        ))
        .unwrap();

        let missing_worktree_review =
            run(["review", task_id.as_str(), "test-agent"], &test_dir.path).unwrap_err();
        let missing_worktree_diff =
            run(["diff", task_id.as_str(), "test-agent"], &test_dir.path).unwrap_err();

        assert!(missing_review
            .to_string()
            .contains("swarm metadata not found"));
        assert!(missing_diff
            .to_string()
            .contains("swarm metadata not found"));
        assert!(invalid_missing_review
            .to_string()
            .contains("unknown swarm agent role"));
        assert!(invalid_missing_diff
            .to_string()
            .contains("unknown swarm agent role"));
        assert!(invalid_review
            .to_string()
            .contains("unknown swarm agent role"));
        assert!(invalid_diff
            .to_string()
            .contains("unknown swarm agent role"));
        assert!(missing_worktree_review
            .to_string()
            .contains("worktree not found"));
        assert!(missing_worktree_diff
            .to_string()
            .contains("worktree not found"));
    }

    #[test]
    fn swarm_review_and_diff_reject_unsafe_role_worktree_path() {
        let test_dir = TestDir::create("swarm-review-unsafe-path");
        initialize_git_repo(&test_dir.path);
        commit_file(&test_dir.path, "tracked.txt", "source\n");
        let description = "Reject unsafe swarm role path";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        replace_swarm_worktree_path_in_metadata(
            &test_dir.path,
            &task_id,
            "security-agent",
            &test_dir.path.join("outside-security-agent"),
        );

        let review_error = run(
            ["review", task_id.as_str(), "security-agent"],
            &test_dir.path,
        )
        .unwrap_err();
        let diff_error =
            run(["diff", task_id.as_str(), "security-agent"], &test_dir.path).unwrap_err();

        assert!(review_error.to_string().contains("safe project path"));
        assert!(diff_error.to_string().contains("safe project path"));
    }

    #[test]
    fn swarm_review_and_diff_preserve_read_only_behavior() {
        let test_dir = TestDir::create("swarm-review-readonly");
        initialize_git_repo(&test_dir.path);
        fs::create_dir_all(test_dir.path.join("src")).unwrap();
        commit_file(&test_dir.path, "src/app.tsx", "before\n");
        let description = "Swarm read-only review";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan read-only review.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        let frontend_worktree =
            swarm_agent_worktree_path(&test_dir.path, &task_id, "frontend-agent");
        fs::write(frontend_worktree.join("src/app.tsx"), "after\n").unwrap();
        run_git(&frontend_worktree, &["add", "src/app.tsx"]);

        let metadata_path = swarm_metadata_path(&test_dir.path, &task_id);
        let before_metadata = fs::read_to_string(&metadata_path).unwrap();
        let before_status = git_text(&frontend_worktree, &["status", "--short"]);
        let before_unstaged_diff = git_text(&frontend_worktree, &["diff"]);
        let before_staged_diff = git_text(&frontend_worktree, &["diff", "--cached"]);

        run(
            ["review", task_id.as_str(), "frontend-agent"],
            &test_dir.path,
        )
        .unwrap();
        run(["diff", task_id.as_str(), "frontend-agent"], &test_dir.path).unwrap();

        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_metadata);
        assert_eq!(
            git_text(&frontend_worktree, &["status", "--short"]),
            before_status
        );
        assert_eq!(
            git_text(&frontend_worktree, &["diff"]),
            before_unstaged_diff
        );
        assert_eq!(
            git_text(&frontend_worktree, &["diff", "--cached"]),
            before_staged_diff
        );
        assert_eq!(
            fs::read_to_string(test_dir.path.join("src/app.tsx")).unwrap(),
            "before\n"
        );
    }

    #[test]
    fn test_task_detects_and_runs_cargo_tests_in_task_worktree() {
        let test_dir = TestDir::create("test-task-detect-cargo");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Run task tests";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let worktree_path = task_worktree_path(&test_dir.path, &task_id);
        let before_source = fs::read_to_string(worktree_path.join("src/lib.rs")).unwrap();
        let metadata_path = task_metadata_path(&test_dir.path, &task_id);
        let before_metadata = fs::read_to_string(&metadata_path).unwrap();
        let output = run(["test", task_id.as_str()], &test_dir.path).unwrap();

        assert!(output.contains("RaptorCode Test Run"));
        assert!(output.contains("Mode: single-task"));
        assert!(output.contains("Targets: 1"));
        assert!(output.contains("Summary: 1/1 passed"));
        assert!(output.contains("Command: cargo test"));
        assert!(output.contains("Command source: detected Cargo.toml"));
        assert!(output.contains("Status: passed"));
        assert!(output.contains("Dependency installation: not performed"));
        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_metadata);
        assert_eq!(
            fs::read_to_string(worktree_path.join("src/lib.rs")).unwrap(),
            before_source
        );
    }

    #[test]
    fn test_task_supports_explicit_test_command() {
        let test_dir = TestDir::create("test-task-explicit");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Run explicit task tests";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let output = run(
            ["test", task_id.as_str(), "--", "cargo", "test", "--quiet"],
            &test_dir.path,
        )
        .unwrap();

        assert!(output.contains("Command: cargo test --quiet"));
        assert!(output.contains("Command source: explicit CLI command"));
        assert!(output.contains("Summary: 1/1 passed"));
    }

    #[test]
    fn test_task_reports_failing_command_with_captured_output() {
        let test_dir = TestDir::create("test-task-failure");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Run failing task tests";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let error = run(
            [
                "test",
                task_id.as_str(),
                "--",
                "cargo",
                "test",
                "--manifest-path",
                "missing-Cargo.toml",
            ],
            &test_dir.path,
        )
        .unwrap_err();
        let output = error.output().unwrap();

        assert_eq!(error.to_string(), "test command failed");
        assert!(output.contains("Status: failed"));
        assert!(output.contains("Exit code:"));
        assert!(output.contains("Stderr:"));
        assert!(output.contains("missing-Cargo.toml"));
    }

    #[test]
    fn test_task_rejects_missing_detected_command_and_unsafe_explicit_command() {
        let test_dir = TestDir::create("test-task-no-command");
        initialize_git_repo(&test_dir.path);
        let description = "Run missing test command";
        let task_id = TaskId::from_description(description).unwrap();

        run(["task", description], &test_dir.path).unwrap();
        let missing = run(["test", task_id.as_str()], &test_dir.path).unwrap_err();
        let blocked = run(
            ["test", task_id.as_str(), "--", "npm", "install"],
            &test_dir.path,
        )
        .unwrap_err();
        let shell_blocked = run(
            [
                "test",
                task_id.as_str(),
                "--",
                "bash",
                "-lc",
                "touch SHOULD_NOT_EXIST",
            ],
            &test_dir.path,
        )
        .unwrap_err();
        let traversal_blocked = run(
            [
                "test",
                task_id.as_str(),
                "--",
                "cargo",
                "test",
                "--manifest-path",
                "../Cargo.toml",
            ],
            &test_dir.path,
        )
        .unwrap_err();

        assert!(missing.to_string().contains("no test command detected"));
        assert!(missing
            .output()
            .unwrap()
            .contains("blocked_no_test_command_detected"));
        assert!(blocked.to_string().contains("not allowed"));
        assert!(shell_blocked.to_string().contains("allowed test command"));
        assert!(traversal_blocked
            .to_string()
            .contains("safe test command argument"));
        assert!(!task_worktree_path(&test_dir.path, &task_id)
            .join("SHOULD_NOT_EXIST")
            .exists());
    }

    #[test]
    fn test_swarm_runs_test_command_in_all_role_worktrees() {
        let test_dir = TestDir::create("test-swarm-all");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Run swarm tests";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan tests.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        let metadata_path = swarm_metadata_path(&test_dir.path, &task_id);
        let before_metadata = fs::read_to_string(&metadata_path).unwrap();
        let before_sources = SwarmAgentRole::ALL
            .iter()
            .map(|role| {
                let role = role.as_str();
                (
                    role,
                    fs::read_to_string(
                        swarm_agent_worktree_path(&test_dir.path, &task_id, role)
                            .join("src/lib.rs"),
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let output = run(
            ["test", task_id.as_str(), "--", "cargo", "test", "--quiet"],
            &test_dir.path,
        )
        .unwrap();

        assert!(output.contains("Mode: swarm"));
        assert!(output.contains("Targets: 4"));
        assert!(output.contains("Summary: 4/4 passed"));
        assert!(output.contains("Target: frontend-agent"));
        assert!(output.contains("Target: backend-agent"));
        assert!(output.contains("Target: test-agent"));
        assert!(output.contains("Target: security-agent"));
        assert_eq!(fs::read_to_string(metadata_path).unwrap(), before_metadata);
        for (role, before_source) in before_sources {
            assert_eq!(
                fs::read_to_string(
                    swarm_agent_worktree_path(&test_dir.path, &task_id, role).join("src/lib.rs")
                )
                .unwrap(),
                before_source
            );
        }
    }

    #[test]
    fn test_task_rejects_unsafe_ids_missing_metadata_and_missing_worktrees() {
        let test_dir = TestDir::create("test-task-rejects");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Reject test worktree";
        let task_id = TaskId::from_description(description).unwrap();

        let unsafe_error = run(["test", "../secret"], &test_dir.path).unwrap_err();
        let missing_error = run(["test", "missing-task"], &test_dir.path).unwrap_err();

        run(["task", description], &test_dir.path).unwrap();
        fs::remove_dir_all(task_worktree_path(&test_dir.path, &task_id)).unwrap();
        let missing_worktree = run(["test", task_id.as_str()], &test_dir.path).unwrap_err();

        assert!(unsafe_error.to_string().contains("not safe"));
        assert!(missing_error
            .to_string()
            .contains("swarm metadata not found"));
        assert!(missing_worktree.to_string().contains("worktree not found"));
    }

    #[test]
    fn test_swarm_rejects_unsafe_role_worktree_path() {
        let test_dir = TestDir::create("test-swarm-unsafe-path");
        initialize_git_repo(&test_dir.path);
        commit_cargo_fixture(&test_dir.path);
        let description = "Reject unsafe swarm test path";
        let task_id = TaskId::from_description(description).unwrap();
        let provider = FakeDeepSeekProvider::new("Plan first.");

        create_swarm_with_planning_source(
            &test_dir.path,
            description,
            SwarmPlanningSource::Provider(&provider),
        )
        .unwrap();
        replace_swarm_worktree_path_in_metadata(
            &test_dir.path,
            &task_id,
            "security-agent",
            &test_dir.path.join("outside-security-agent"),
        );

        let error = run(
            ["test", task_id.as_str(), "--", "cargo", "test", "--quiet"],
            &test_dir.path,
        )
        .unwrap_err();

        assert!(error.to_string().contains("safe project path"));
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
}
