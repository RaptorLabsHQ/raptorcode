//! RaptorCode command-line interface: argument parsing, the interactive REPL,
//! the live swarm orchestrator, and tool-execution policy.

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io::{BufRead, Write};
use std::path::Path;

mod agent_loop;
mod agent_tools;
mod context_budget;
mod prompt_guard;
mod prompts;
mod render;
mod repl;
mod slash_command;
mod swarm_orchestrator;
mod task_commands;
mod tool_policy;

// The crate's public API: the dispatcher below plus the command handlers and
// renderers that the binaries and the integration tests call directly.
pub use render::render_doctor;
pub use repl::run_interactive_chat;
pub use task_commands::{
    abandon_task, cleanup_task, create_swarm, create_task, diff_task, implement_swarm_role,
    list_tasks, merge_task, merge_task_execute, review_task, show_task, test_task,
};

use prompt_guard::PromptGuard;
use raptor_agent::{AgentId, AgentRole, AgentSession, PermissionPolicy};
use raptor_core::{project_description, version};
use raptor_git::{inspect_repository_root, GitError};
use raptor_providers::{
    AgentMessage, CompletionMessage, CompletionRequest, DeepSeekProvider, ModelId, Provider,
    ProviderCatalog, ProviderConfiguration, ProviderError, ProviderId, DEEPSEEK_API_KEY_URL,
};
use raptor_store::{
    init_project_config, list_agent_sessions, list_swarm_metadata, list_task_metadata,
    persist_agent_session, read_agent_session, read_project_config, read_task_metadata,
    InitConfigOutcome, ProjectConfigError, TaskStoreError,
};
use raptor_tui::{
    color_mode_from_environment, render_dashboard_snapshot, render_startup_banner, run_dashboard,
    terminal_width_from_environment, AgentDashboardSummary, ColorMode, DashboardState,
    StartupBannerOptions, SwarmDashboardSummary, TaskDashboardSummary,
};
use render::{
    project_config_dir, render_agent_created, render_agent_list, render_agent_session,
    render_chat_response, render_permission_policy, render_provider_list,
};
use task_commands::cmd_swarm_run;

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
    SwarmRun {
        description: String,
    },
    Task(TaskCommand),
    Test {
        task_id: String,
        test_command: Option<Vec<String>>,
    },
    Tui,
    Startup,
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
const SWARM_MODEL_ID: &str = "deepseek-v4-pro";
const RAPTOR_PLAN_FILE_NAME: &str = "RAPTOR_PLAN.md";
const RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME: &str = "RAPTOR_IMPLEMENTATION_PROPOSAL.md";

pub(crate) const C_RESET: &str = "\x1b[0m";
pub(crate) const C_BOLD: &str = "\x1b[1m";
pub(crate) const C_DIM: &str = "\x1b[2m";
pub(crate) const C_RED: &str = "\x1b[91m";
pub(crate) const C_RED_BOLD: &str = "\x1b[1;91m";
pub(crate) const C_GREEN_BOLD: &str = "\x1b[1;32m";
const C_GREEN: &str = "\x1b[32m";
const C_YELLOW_BOLD: &str = "\x1b[1;33m";
pub(crate) const C_CYAN: &str = "\x1b[36m";

pub(crate) struct MenuItem {
    pub(crate) label: String,
    pub(crate) sublabel: Option<String>,
    pub(crate) enabled: bool,
}

fn render_menu_lines(items: &[MenuItem], cursor: usize, raw_mode: bool) {
    use std::io::Write as _;
    let nl: &str = if raw_mode { "\r\n" } else { "\n" };
    for (i, item) in items.iter().enumerate() {
        let selected = i == cursor;
        if selected && item.enabled {
            print!(
                "  {C_GREEN_BOLD}❯{C_RESET}  {C_BOLD}{}{C_RESET}",
                item.label
            );
        } else if item.enabled {
            print!("     {}", item.label);
        } else {
            print!("     {C_DIM}{}{C_RESET}", item.label);
        }
        if let Some(ref sub) = item.sublabel {
            print!("  {C_DIM}{sub}{C_RESET}");
        }
        print!("{nl}");
    }
    std::io::stdout().flush().ok();
}

pub(crate) fn interactive_select(items: &[MenuItem]) -> Option<usize> {
    use crossterm::{
        event::{self, Event, KeyCode, KeyModifiers},
        terminal::{disable_raw_mode, enable_raw_mode},
    };

    let enabled: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.enabled)
        .map(|(i, _)| i)
        .collect();

    if enabled.is_empty() {
        return None;
    }

    let mut cursor_idx = 0usize;
    render_menu_lines(items, enabled[cursor_idx], false);

    if enable_raw_mode().is_err() {
        return enabled.first().copied();
    }

    let result = loop {
        let key = match event::read() {
            Ok(Event::Key(key)) => key,
            Ok(_) => continue,
            // An unreadable input stream (closed terminal, broken pipe) must end
            // the menu rather than spin forever in raw mode.
            Err(_) => break None,
        };
        match key.code {
            KeyCode::Up if cursor_idx > 0 => {
                cursor_idx -= 1;
                print!("\x1b[{}A\r", items.len());
                std::io::stdout().flush().ok();
                render_menu_lines(items, enabled[cursor_idx], true);
            }
            KeyCode::Down if cursor_idx + 1 < enabled.len() => {
                cursor_idx += 1;
                print!("\x1b[{}A\r", items.len());
                std::io::stdout().flush().ok();
                render_menu_lines(items, enabled[cursor_idx], true);
            }
            KeyCode::Enter => break Some(enabled[cursor_idx]),
            KeyCode::Esc => break None,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break None,
            _ => {}
        }
    };

    disable_raw_mode().ok();
    println!();
    result
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

/// Every lower-layer error converts into a `CliError` carrying its `Display`
/// text, so command handlers use `?` instead of mapping each call by hand.
macro_rules! cli_error_from {
    ($($error:ty),* $(,)?) => {$(
        impl From<$error> for CliError {
            fn from(error: $error) -> Self {
                CliError::new(error.to_string())
            }
        }
    )*};
}

cli_error_from!(
    GitError,
    ProviderError,
    TaskStoreError,
    ProjectConfigError,
    raptor_agent::AgentError,
    raptor_core::TaskError,
    raptor_store::AgentStoreError,
    raptor_store::InitConfigError,
    raptor_store::SwarmStoreError,
    raptor_tools::TestCommandError,
    std::io::Error,
);

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
        [] => Ok(CliCommand::Startup),
        [arg] if arg == "--help" || arg == "-h" => Ok(CliCommand::Help),
        [arg] if arg == "--version" || arg == "-V" => Ok(CliCommand::Version),
        [arg] if arg == "init" => Ok(CliCommand::Init),
        [arg] if arg == "tui" => Ok(CliCommand::Tui),
        [arg, ..] if arg == "tui" => Err(CliError::new("tui accepts no additional arguments")),
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
        [arg, subcommand] if arg == "swarm" && subcommand == "run" => {
            Err(CliError::new("swarm run mission description is required"))
        }
        [arg, subcommand, description @ ..] if arg == "swarm" && subcommand == "run" => {
            Ok(CliCommand::SwarmRun {
                description: description.join(" "),
            })
        }
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
    let startup_options = startup_banner_options(current_dir, false, true, 80, ColorMode::None);
    run_with_startup_banner(args, current_dir, confirmation_input, startup_options)
}

pub fn run_with_startup_banner<I, S>(
    args: I,
    current_dir: &Path,
    confirmation_input: Option<&mut dyn BufRead>,
    startup_options: StartupBannerOptions,
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
        CliCommand::SwarmRun { description } => cmd_swarm_run(current_dir, &description),
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
        CliCommand::Tui => Ok(render_dashboard_snapshot(&dashboard_state(current_dir))),
        CliCommand::Startup => Ok(render_startup_banner(&startup_options)),
        CliCommand::Version => Ok(render_version()),
    }
}

pub fn startup_banner_options(
    current_dir: &Path,
    interactive: bool,
    ci: bool,
    terminal_width: u16,
    color_mode: ColorMode,
) -> StartupBannerOptions {
    StartupBannerOptions {
        api_key_url: DEEPSEEK_API_KEY_URL.to_owned(),
        ci,
        color_mode,
        compact: terminal_width < 80,
        cwd: current_dir.to_path_buf(),
        deepseek_api_key_present: crate::repl::resolve_deepseek_api_key().is_some(),
        emoji: emoji_environment_enabled(),
        interactive,
        mode: Some("high effort".to_owned()),
        model_name: Some("DeepSeek".to_owned()),
        terminal_width,
        user_name: "developer".to_owned(),
        version: version().to_owned(),
    }
}

pub(crate) fn format_model_display_name(id: &str) -> String {
    let stripped = id.strip_prefix("deepseek-").unwrap_or(id);
    let parts: Vec<String> = stripped
        .split('-')
        .map(|part| {
            if part.starts_with('v') && part[1..].chars().all(|c| c.is_ascii_digit()) {
                part.to_ascii_uppercase()
            } else {
                let mut chars = part.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            }
        })
        .collect();
    format!("DeepSeek {}", parts.join(" "))
}

fn pick_latest_model(models: &[String]) -> Option<String> {
    models
        .iter()
        .filter(|m| m.starts_with("deepseek"))
        .max_by_key(|m| {
            let ver = m
                .find("-v")
                .and_then(|p| {
                    m[p + 2..]
                        .split('-')
                        .next()
                        .and_then(|s| s.parse::<i32>().ok())
                })
                .unwrap_or(0)
                * 100;
            let tier = if m.contains("pro") {
                20
            } else if m.contains("reasoner") {
                15
            } else if m.contains("chat") {
                10
            } else if m.contains("flash") {
                5
            } else {
                1
            };
            ver + tier
        })
        .cloned()
}

pub fn runtime_startup_banner_options(
    current_dir: &Path,
    stdout_interactive: bool,
) -> StartupBannerOptions {
    let ci = ci_environment_enabled();
    let terminal_width = terminal_width_from_environment();
    let color_mode = color_mode_from_environment(stdout_interactive, ci);

    startup_banner_options(
        current_dir,
        stdout_interactive,
        ci,
        terminal_width,
        color_mode,
    )
}

pub fn dashboard_state(current_dir: &Path) -> DashboardState {
    let mut state = DashboardState::empty(version(), current_dir);
    state.deepseek_api_key_present = crate::repl::resolve_deepseek_api_key().is_some();
    state.provider_name = SWARM_PROVIDER_DISPLAY.to_owned();
    state.model_name = SWARM_MODEL_ID.to_owned();
    state.test_status = Some("not recorded".to_owned());

    if let Ok(repository_root) = inspect_repository_root(current_dir) {
        let root = repository_root.path();
        if let Ok(tasks) = list_task_metadata(root) {
            state.tasks = tasks
                .into_iter()
                .map(|task| TaskDashboardSummary {
                    id: task.id,
                    status: task
                        .status
                        .map_or("-".to_owned(), |status| status.as_str().to_owned()),
                })
                .collect();
        }
        if let Ok(swarms) = list_swarm_metadata(root) {
            state.swarms = swarms
                .iter()
                .map(|swarm| SwarmDashboardSummary {
                    id: swarm.id.clone(),
                    planning_status: swarm
                        .planning_status
                        .map(|status| status.as_str().to_owned()),
                    status: swarm.status.as_str().to_owned(),
                })
                .collect();

            if let Some(latest_swarm) = swarms.last() {
                state.agent_roles = latest_swarm
                    .agents
                    .iter()
                    .map(|agent| AgentDashboardSummary {
                        planning_status: agent
                            .planning_status
                            .map(|status| status.as_str().to_owned()),
                        proposal_status: agent
                            .implementation_status
                            .map(|status| status.as_str().to_owned()),
                        role: agent.role.clone(),
                        status: agent.status.as_str().to_owned(),
                    })
                    .collect();
            }
        }
    }

    state
}

pub fn run_tui_dashboard(current_dir: &Path) -> Result<(), CliError> {
    Ok(run_dashboard(dashboard_state(current_dir))?)
}

pub fn ci_environment_enabled() -> bool {
    std::env::var("CI")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn emoji_environment_enabled() -> bool {
    std::env::var("RAPTORCODE_NO_EMOJI")
        .map(|value| {
            !matches!(
                value.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(true)
}

pub fn render_version() -> String {
    format!("raptor {}\n", version())
}

pub fn render_help() -> String {
    format!(
        "raptor {}\n\n{}\n\nUsage:\n  raptor\n  raptor --version\n  raptor init\n  raptor tui\n  raptor provider list\n  raptor agent list\n  raptor agent show <id>\n  raptor agent create --role <role> [--task <task-id>] [--provider <provider-id>] [--model <model-id>]\n  raptor agent policy\n  raptor chat \"message\"\n  raptor task \"task description\"\n  raptor task list\n  raptor task show <id>\n  raptor task abandon <id>\n  raptor task cleanup <id>\n  raptor swarm \"task description\"\n  raptor swarm implement <task-id> <role>\n  raptor swarm run \"mission description\"\n  raptor review <task-id> [role]\n  raptor diff <task-id> [role]\n  raptor test <task-id> [-- <command>]\n  raptor merge <task-id>\n  raptor merge <task-id> --execute\n  raptor doctor\n",
        version(),
        project_description()
    )
}

pub fn init_project(current_dir: &Path) -> Result<String, CliError> {
    match init_project_config(current_dir)? {
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
                    ProviderId::new(provider.id.clone()).map(|provider_id| {
                        ProviderConfiguration::new(provider_id, provider.enabled)
                    })
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
    let provider = DeepSeekProvider::from_env()?;

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
            CompletionMessage::system(prompts::chat_system_prompt(false)),
            CompletionMessage::user(message),
        ],
    );
    let mut response = provider.complete(request)?;
    response.content = PromptGuard::from_prompts().redact(&response.content);

    Ok(render_chat_response(&response))
}

pub(crate) fn chat_system_prompt(json_mode: bool) -> String {
    prompts::chat_system_prompt(json_mode)
}

pub(crate) fn agent_system_message(current_dir: &Path, json_mode: bool) -> AgentMessage {
    let content = prompts::agent_system_prompt(&current_dir.display().to_string(), json_mode);

    AgentMessage::system(content)
}

pub fn create_agent(
    current_dir: &Path,
    role: &str,
    task_id: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let role = AgentRole::parse(role)?;
    let provider_id = provider_id
        .map(|provider_id| {
            ProviderId::new(provider_id).map(|provider_id| provider_id.as_str().to_owned())
        })
        .transpose()?;
    let model_id = model_id
        .map(|model_id| ModelId::new(model_id).map(|model_id| model_id.as_str().to_owned()))
        .transpose()?;
    let task = task_id
        .as_deref()
        .map(|task_id| read_task_metadata(repository_root.path(), task_id))
        .transpose()?;
    let agent_id = AgentId::from_session_parts(
        role,
        task_id.as_deref(),
        provider_id.as_deref(),
        model_id.as_deref(),
    )?;
    let mut session = AgentSession::new_read_only(agent_id, role);

    if let Some(provider_id) = provider_id {
        session.set_provider_id(provider_id)?;
    }
    if let Some(model_id) = model_id {
        session.set_model_id(model_id)?;
    }
    if let Some(task) = task.as_ref() {
        let worktree_path = task
            .worktree_path
            .clone()
            .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
        session.bind_task(task.id.clone(), worktree_path)?;
    }

    let metadata_path = persist_agent_session(repository_root.path(), &session)?;
    let stored = read_agent_session(repository_root.path(), session.id.as_str())?;

    Ok(render_agent_created(&stored, &metadata_path))
}

pub fn list_agents(current_dir: &Path) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let sessions = list_agent_sessions(repository_root.path())?;

    Ok(render_agent_list(&sessions))
}

pub fn show_agent(current_dir: &Path, agent_id: &str) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let session = read_agent_session(repository_root.path(), agent_id)?;

    Ok(render_agent_session(&session))
}

#[cfg(test)]
mod tests;
