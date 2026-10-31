use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use raptor_core::{SwarmAgentRole, SwarmStatus, TaskId};
use raptor_git::create_swarm_worktrees;
use raptor_providers::{AgentMessage, DeepSeekProvider, DeepSeekRequestOptions, ModelId};
use raptor_store::{persist_swarm_metadata, update_swarm_metadata};
use raptor_tui::{run_swarm_dashboard, SwarmDashboardState, SwarmUiEvent};

use crate::agent_loop::{self, AgentLoopEnd, AgentLoopEvent, AgentLoopUi, ToolOutcome};
use crate::agent_tools::build_agent_tools;
use crate::context_budget;
use crate::tool_policy::{ConfirmationRequest, ToolPolicy};
use crate::CliError;

pub(crate) const SWARM_MAX_STEPS: u32 = agent_loop::MAX_STEPS;
const AGENT_SPAWN_STAGGER: Duration = Duration::from_millis(300);
const SUMMARY_EVENT_MAX_CHARS: usize = 200;

pub(crate) struct LiveSwarmConfig {
    pub api_key: String,
    pub mission: String,
    pub model_id: ModelId,
    pub model_label: String,
    pub request_options: DeepSeekRequestOptions,
}

pub(crate) struct SwarmAgentOutcome {
    pub aborted: bool,
    pub completed: bool,
    pub files_written: usize,
    pub role: String,
    pub steps: u32,
    pub summary: String,
    pub worktree: PathBuf,
}

struct SwarmWorkerConfig {
    agent_index: usize,
    api_key: String,
    cancel: Arc<AtomicBool>,
    events: Sender<SwarmUiEvent>,
    mission: String,
    model_id: ModelId,
    request_options: DeepSeekRequestOptions,
    role: String,
    worktree: PathBuf,
}

fn swarm_mission_message(mission: &str, role: &str) -> String {
    format!(
        "Mission for the whole swarm: {mission}\n\nYou are the {role}. Deliver your specialty's share of this mission inside your worktree now."
    )
}

fn tool_call_detail(arguments: &str) -> Option<String> {
    let args = serde_json::from_str::<serde_json::Value>(arguments).ok()?;
    args["path"]
        .as_str()
        .or_else(|| args["working_dir"].as_str())
        .map(str::to_owned)
}

/// Flattens an agent's final message into one line for the dashboard and the
/// post-run summary: the model answers in markdown, and its newlines, heading
/// markers, and bold markers would otherwise break the indented layout.
fn truncate_summary(text: &str) -> String {
    let flattened = text
        .split_whitespace()
        .filter(|word| !word.chars().all(|character| character == '#'))
        .collect::<Vec<_>>()
        .join(" ")
        .replace("**", "");
    if flattened.chars().count() <= SUMMARY_EVENT_MAX_CHARS {
        return flattened;
    }
    let cut: String = flattened
        .chars()
        .take(SUMMARY_EVENT_MAX_CHARS - 1)
        .collect();
    format!("{cut}…")
}

/// Creates the swarm worktrees, spawns one live specialist agent per role on
/// its own thread, runs the dashboard on the calling thread, and records the
/// outcome in swarm metadata. Returns the post-run developer summary.
pub(crate) fn run_live_swarm(
    current_dir: &Path,
    config: LiveSwarmConfig,
) -> Result<String, CliError> {
    let mission = config.mission.trim().to_owned();
    let task_id = TaskId::from_description(&mission)?;
    let repository_root = raptor_git::inspect_repository_root(current_dir)
        .map_err(|error| CliError::new(format!("live swarm requires a git repository: {error}")))?;

    if raptor_store::task_metadata_exists(repository_root.path(), &task_id)?
        || raptor_store::swarm_metadata_exists(repository_root.path(), &task_id)?
    {
        return Err(CliError::new(format!(
            "task or swarm metadata already exists for {}; choose a different mission description",
            task_id.as_str()
        )));
    }

    let plan = create_swarm_worktrees(&repository_root, &task_id)?;
    let mut metadata = crate::task_commands::swarm_metadata_from_plan(&plan, &mission);
    metadata.status = SwarmStatus::LiveRunning;
    for agent in &mut metadata.agents {
        agent.status = SwarmStatus::LiveRunning;
        agent.planning_status = None;
    }
    persist_swarm_metadata(repository_root.path(), &metadata)?;

    let roles: Vec<String> = plan.agents.iter().map(|agent| agent.role.clone()).collect();
    let dashboard_state = SwarmDashboardState::new(
        mission.clone(),
        config.model_label.clone(),
        &roles,
        SWARM_MAX_STEPS,
        crate::repl::swarm_quote_pool(),
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let (event_tx, event_rx) = channel::<SwarmUiEvent>();

    let mut worker_handles = Vec::with_capacity(plan.agents.len());
    for (agent_index, agent_plan) in plan.agents.iter().enumerate() {
        let worker = SwarmWorkerConfig {
            agent_index,
            api_key: config.api_key.clone(),
            cancel: Arc::clone(&cancel),
            events: event_tx.clone(),
            mission: mission.clone(),
            model_id: config.model_id.clone(),
            request_options: config.request_options.clone(),
            role: agent_plan.role.clone(),
            worktree: agent_plan.path.clone(),
        };
        if agent_index > 0 {
            thread::sleep(AGENT_SPAWN_STAGGER);
        }
        worker_handles.push(thread::spawn(move || run_swarm_agent_worker(worker)));
    }
    drop(event_tx);

    // Run the dashboard on this thread. On dashboard failure we must still tear
    // every worker down cleanly, so signal cancellation and remember the error
    // rather than returning before the join loop.
    let (aborted, dashboard_error) =
        match run_swarm_dashboard(dashboard_state, event_rx, Arc::clone(&cancel)) {
            Ok(aborted) => (aborted, None),
            Err(error) => {
                cancel.store(true, Ordering::Relaxed);
                (true, Some(format!("swarm dashboard failed: {error}")))
            }
        };

    // Join every worker so none is left detached mid-run. A panicked worker is
    // recorded as failed and does not abort the rest of the teardown.
    let mut outcomes = Vec::with_capacity(worker_handles.len());
    let mut panicked_roles: Vec<String> = Vec::new();
    for (handle, role) in worker_handles.into_iter().zip(roles.iter()) {
        match handle.join() {
            Ok(outcome) => outcomes.push(outcome),
            Err(_) => panicked_roles.push(role.clone()),
        }
    }

    for outcome in &outcomes {
        if let Some(agent) = metadata
            .agents
            .iter_mut()
            .find(|agent| agent.role == outcome.role)
        {
            let status = if outcome.completed {
                SwarmStatus::LiveCompleted
            } else if outcome.aborted {
                SwarmStatus::LiveAborted
            } else {
                SwarmStatus::LiveFailed
            };
            agent.status = status;
            agent.implementation_status = Some(status);
        }
    }
    for role in &panicked_roles {
        if let Some(agent) = metadata.agents.iter_mut().find(|agent| &agent.role == role) {
            agent.status = SwarmStatus::LiveFailed;
            agent.implementation_status = Some(SwarmStatus::LiveFailed);
        }
    }

    metadata.status =
        if panicked_roles.is_empty() && outcomes.iter().all(|outcome| outcome.completed) {
            SwarmStatus::LiveCompleted
        } else if aborted || outcomes.iter().any(|outcome| outcome.aborted) {
            SwarmStatus::LiveAborted
        } else {
            SwarmStatus::LiveFailed
        };
    // Always persist the final status so a panic or dashboard failure can never
    // leave the swarm wedged at "running".
    update_swarm_metadata(repository_root.path(), &metadata)?;

    if let Some(error) = dashboard_error {
        return Err(CliError::new(error));
    }
    if !panicked_roles.is_empty() {
        return Err(CliError::new(format!(
            "swarm agent thread(s) panicked ({}); final status persisted, inspect the worktrees manually",
            panicked_roles.join(", ")
        )));
    }

    Ok(render_live_swarm_summary(task_id.as_str(), &outcomes))
}

fn render_live_swarm_summary(task_id: &str, outcomes: &[SwarmAgentOutcome]) -> String {
    let mut output = String::new();
    output.push_str(&format!("\nSwarm run finished — task {task_id}\n\n"));
    for outcome in outcomes {
        let marker = if outcome.completed {
            "✓"
        } else if outcome.aborted {
            "⏸"
        } else {
            "✗"
        };
        output.push_str(&format!(
            "  {marker} {:<16} {} files · {} steps · {}\n",
            outcome.role,
            outcome.files_written,
            outcome.steps,
            outcome.worktree.display()
        ));
        if !outcome.summary.is_empty() {
            output.push_str(&format!("      {}\n", outcome.summary));
        }
    }
    output.push_str(&format!(
        "\nAll changes are uncommitted inside the role worktrees for your review.\nNext: raptor review {task_id} [role] · raptor diff {task_id} [role] · raptor test {task_id}\nThen commit inside a role worktree and run: raptor merge {task_id} --execute\n"
    ));
    output
}

fn run_swarm_agent_worker(config: SwarmWorkerConfig) -> SwarmAgentOutcome {
    let mut outcome = SwarmAgentOutcome {
        aborted: false,
        completed: false,
        files_written: 0,
        role: config.role.clone(),
        steps: 0,
        summary: String::new(),
        worktree: config.worktree.clone(),
    };

    let provider = match DeepSeekProvider::new(&config.api_key) {
        Ok(provider) => provider,
        Err(error) => {
            let _ = config.events.send(SwarmUiEvent::AgentFailed {
                agent: config.agent_index,
                error: format!("provider init failed: {error}"),
            });
            outcome.summary = format!("provider init failed: {error}");
            return outcome;
        }
    };

    let tools = build_agent_tools();
    let workspace_root = config
        .worktree
        .canonicalize()
        .unwrap_or_else(|_| config.worktree.clone());
    let mut policy = ToolPolicy::new(workspace_root.clone());
    let mut working_dir = workspace_root;
    let mut budget = context_budget::ContextBudgetState::default();
    let Some(role) = SwarmAgentRole::parse(&config.role) else {
        let message = format!("unknown swarm role: {}", config.role);
        let _ = config.events.send(SwarmUiEvent::AgentFailed {
            agent: config.agent_index,
            error: message.clone(),
        });
        outcome.summary = message;
        return outcome;
    };
    let mut history = vec![
        AgentMessage::system(crate::prompts::swarm_role_system_prompt(role)),
        AgentMessage::user(swarm_mission_message(&config.mission, &config.role)),
    ];

    let _ = config.events.send(SwarmUiEvent::AgentSpawned {
        agent: config.agent_index,
    });

    let mut ui = SwarmAgentUi {
        agent_index: config.agent_index,
        cancel: &config.cancel,
        events: &config.events,
        files_written: 0,
        in_flight: None,
        steps: 0,
    };
    let mut context = agent_loop::AgentLoopContext {
        budget: &mut budget,
        model_id: &config.model_id,
        provider: &provider,
        request_options: &config.request_options,
        // Swarm workers never stream: the dashboard shows status lines, not text.
        streaming: false,
        tool_policy: &mut policy,
        tools: &tools,
        working_dir: &mut working_dir,
    };
    let end = agent_loop::run(&mut context, &mut history, &mut ui);
    outcome.steps = ui.steps;
    outcome.files_written = ui.files_written;

    match end {
        Ok(AgentLoopEnd::Answered { content, usage, .. }) => {
            if let Some(total) = usage.and_then(|usage| usage.total_tokens) {
                let _ = config.events.send(SwarmUiEvent::AgentUsage {
                    agent: config.agent_index,
                    total_tokens: u64::from(total),
                });
            }
            outcome.completed = true;
            outcome.summary = truncate_summary(&content);
            let _ = config.events.send(SwarmUiEvent::AgentFinished {
                agent: config.agent_index,
                summary: outcome.summary.clone(),
            });
        }
        Ok(AgentLoopEnd::StepLimit) => {
            outcome.completed = true;
            outcome.summary = format!(
                "step limit ({SWARM_MAX_STEPS}) reached; partial work saved in the worktree"
            );
            let _ = config.events.send(SwarmUiEvent::AgentFinished {
                agent: config.agent_index,
                summary: outcome.summary.clone(),
            });
        }
        Ok(AgentLoopEnd::Cancelled) => {
            return finish_aborted(config.agent_index, &config.events, outcome);
        }
        Err(error) => {
            let message = format!("agent error: {error}");
            let _ = config.events.send(SwarmUiEvent::AgentFailed {
                agent: config.agent_index,
                error: message.clone(),
            });
            outcome.summary = message;
        }
    }

    outcome
}

/// Dashboard adapter for the shared agent loop: forwards progress as
/// `SwarmUiEvent`s, round-trips confirmations through a reply channel the
/// dashboard answers with `[y]`/`[n]`, and counts written files for the
/// post-run summary.
struct SwarmAgentUi<'a> {
    agent_index: usize,
    cancel: &'a AtomicBool,
    events: &'a Sender<SwarmUiEvent>,
    files_written: usize,
    /// Tool name and path detail of the call being executed, so a successful
    /// `write_file` can be reported with its path.
    in_flight: Option<(String, Option<String>)>,
    steps: u32,
}

impl SwarmAgentUi<'_> {
    fn send(&self, event: SwarmUiEvent) {
        let _ = self.events.send(event);
    }
}

impl AgentLoopUi for SwarmAgentUi<'_> {
    fn on_event(&mut self, event: AgentLoopEvent) {
        match event {
            AgentLoopEvent::StepStarted { step } => {
                self.steps = step;
                self.send(SwarmUiEvent::AgentStep {
                    agent: self.agent_index,
                    step,
                });
                self.send(SwarmUiEvent::AgentStatus {
                    agent: self.agent_index,
                    line: "Thinking".to_owned(),
                    detail: None,
                });
            }
            AgentLoopEvent::Usage(usage) => {
                if let Some(total) = usage.total_tokens {
                    self.send(SwarmUiEvent::AgentUsage {
                        agent: self.agent_index,
                        total_tokens: u64::from(total),
                    });
                }
            }
            AgentLoopEvent::ToolStarted { action, call } => {
                let detail = tool_call_detail(&call.arguments);
                self.in_flight = Some((call.name, detail.clone()));
                self.send(SwarmUiEvent::AgentStatus {
                    agent: self.agent_index,
                    line: action,
                    detail,
                });
            }
            AgentLoopEvent::ToolFinished { outcome, .. } => {
                if let Some((name, Some(path))) = self.in_flight.take() {
                    if name == "write_file" && outcome == ToolOutcome::Succeeded {
                        self.files_written += 1;
                        self.send(SwarmUiEvent::AgentFileWritten {
                            agent: self.agent_index,
                            path,
                        });
                    }
                }
                self.in_flight = None;
            }
            // Workers do not stream, and narration, step/round boundaries, and
            // budget notices have no slot on the dashboard.
            AgentLoopEvent::ContentDelta(_)
            | AgentLoopEvent::StepFinished
            | AgentLoopEvent::Narration(_)
            | AgentLoopEvent::ToolExecuting { .. }
            | AgentLoopEvent::BudgetNotice(_)
            | AgentLoopEvent::RoundFinished => {}
        }
    }

    fn confirm(&mut self, request: &ConfirmationRequest) -> bool {
        let (reply_tx, reply_rx) = channel::<bool>();
        self.send(SwarmUiEvent::AgentNeedsConfirmation {
            agent: self.agent_index,
            message: request.message.clone(),
            reply: reply_tx,
        });
        reply_rx.recv().unwrap_or(false)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn finish_aborted(
    agent_index: usize,
    events: &Sender<SwarmUiEvent>,
    mut outcome: SwarmAgentOutcome,
) -> SwarmAgentOutcome {
    outcome.aborted = true;
    outcome.summary = "stopped by developer".to_owned();
    let _ = events.send(SwarmUiEvent::AgentFailed {
        agent: agent_index,
        error: outcome.summary.clone(),
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_message_names_role_and_mission() {
        let message = swarm_mission_message("Build a SaaS app", "backend-agent");

        assert!(message.contains("Build a SaaS app"));
        assert!(message.contains("backend-agent"));
    }

    #[test]
    fn tool_call_detail_extracts_path_or_working_dir() {
        assert_eq!(
            tool_call_detail("{\"path\":\"src/main.rs\",\"content\":\"x\"}"),
            Some("src/main.rs".to_owned())
        );
        assert_eq!(
            tool_call_detail("{\"command\":\"cargo build\",\"working_dir\":\"app\"}"),
            Some("app".to_owned())
        );
        assert_eq!(tool_call_detail("{\"command\":\"ls\"}"), None);
        assert_eq!(tool_call_detail("not json"), None);
    }

    #[test]
    fn summaries_are_truncated_for_events() {
        let long = "x".repeat(500);
        let truncated = truncate_summary(&long);

        assert_eq!(truncated.chars().count(), SUMMARY_EVENT_MAX_CHARS);
        assert!(truncated.ends_with('…'));
        assert_eq!(truncate_summary("  short  "), "short");
    }

    #[test]
    fn summaries_are_flattened_to_one_line() {
        let markdown = "Done.\n\n## Summary\n**Files created:**\n- `hello.txt` — contains `hello`";

        assert_eq!(
            truncate_summary(markdown),
            "Done. Summary Files created: - `hello.txt` — contains `hello`"
        );
    }
}
