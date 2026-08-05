use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use raptor_core::{
    TaskId, SWARM_LIVE_RUN_ABORTED_STATUS, SWARM_LIVE_RUN_COMPLETED_STATUS,
    SWARM_LIVE_RUN_FAILED_STATUS, SWARM_LIVE_RUN_RUNNING_STATUS,
};
use raptor_git::create_swarm_worktrees;
use raptor_providers::{
    AgentMessage, AgentStep, AgentStepResult, DeepSeekProvider, DeepSeekRequestOptions, ModelId,
};
use raptor_store::{persist_swarm_metadata, update_swarm_metadata};
use raptor_tui::{run_swarm_dashboard, SwarmDashboardState, SwarmUiEvent};

use crate::agent_tools::{build_agent_tools, describe_tool_call, execute_tool_call};
use crate::context_budget;
use crate::prompt_guard::PromptGuard;
use crate::tool_policy::{ConfirmationKind, PolicyDecision, ToolPolicy};
use crate::CliError;

pub(crate) const SWARM_MAX_STEPS: u32 = 100;
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

pub(crate) fn swarm_role_system_prompt(role: &str) -> String {
    crate::prompts::swarm_role_system_prompt(role)
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

fn truncate_summary(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= SUMMARY_EVENT_MAX_CHARS {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(SUMMARY_EVENT_MAX_CHARS - 1).collect();
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
    let task_id =
        TaskId::from_description(&mission).map_err(|error| CliError::new(error.to_string()))?;
    let repository_root = raptor_git::inspect_repository_root(current_dir)
        .map_err(|error| CliError::new(format!("live swarm requires a git repository: {error}")))?;

    if raptor_store::task_metadata_exists(repository_root.path(), &task_id)
        .map_err(|error| CliError::new(error.to_string()))?
        || raptor_store::swarm_metadata_exists(repository_root.path(), &task_id)
            .map_err(|error| CliError::new(error.to_string()))?
    {
        return Err(CliError::new(format!(
            "task or swarm metadata already exists for {}; choose a different mission description",
            task_id.as_str()
        )));
    }

    let plan = create_swarm_worktrees(&repository_root, &task_id)
        .map_err(|error| CliError::new(error.to_string()))?;
    let mut metadata = crate::swarm_metadata_from_plan(&plan, &mission);
    metadata.status = SWARM_LIVE_RUN_RUNNING_STATUS.to_owned();
    for agent in &mut metadata.agents {
        agent.status = SWARM_LIVE_RUN_RUNNING_STATUS.to_owned();
        agent.planning_status = None;
    }
    persist_swarm_metadata(repository_root.path(), &metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

    let roles: Vec<String> = plan.agents.iter().map(|agent| agent.role.clone()).collect();
    let dashboard_state = SwarmDashboardState::new(
        mission.clone(),
        config.model_label.clone(),
        &roles,
        SWARM_MAX_STEPS,
        crate::swarm_quote_pool(),
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
                SWARM_LIVE_RUN_COMPLETED_STATUS
            } else if outcome.aborted {
                SWARM_LIVE_RUN_ABORTED_STATUS
            } else {
                SWARM_LIVE_RUN_FAILED_STATUS
            };
            agent.status = status.to_owned();
            agent.implementation_status = Some(status.to_owned());
        }
    }
    for role in &panicked_roles {
        if let Some(agent) = metadata.agents.iter_mut().find(|agent| &agent.role == role) {
            agent.status = SWARM_LIVE_RUN_FAILED_STATUS.to_owned();
            agent.implementation_status = Some(SWARM_LIVE_RUN_FAILED_STATUS.to_owned());
        }
    }

    metadata.status =
        if panicked_roles.is_empty() && outcomes.iter().all(|outcome| outcome.completed) {
            SWARM_LIVE_RUN_COMPLETED_STATUS.to_owned()
        } else if aborted || outcomes.iter().any(|outcome| outcome.aborted) {
            SWARM_LIVE_RUN_ABORTED_STATUS.to_owned()
        } else {
            SWARM_LIVE_RUN_FAILED_STATUS.to_owned()
        };
    // Always persist the final status so a panic or dashboard failure can never
    // leave the swarm wedged at "running".
    update_swarm_metadata(repository_root.path(), &metadata)
        .map_err(|error| CliError::new(error.to_string()))?;

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
    let guard = PromptGuard::from_prompts();
    let workspace_root = config
        .worktree
        .canonicalize()
        .unwrap_or_else(|_| config.worktree.clone());
    let mut policy = ToolPolicy::new(workspace_root.clone());
    let mut working_dir = workspace_root;
    let mut budget = context_budget::ContextBudgetState::default();
    let mut history = vec![
        AgentMessage {
            role: "system".to_owned(),
            content: Some(swarm_role_system_prompt(&config.role)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
        AgentMessage {
            role: "user".to_owned(),
            content: Some(swarm_mission_message(&config.mission, &config.role)),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    let _ = config.events.send(SwarmUiEvent::AgentSpawned {
        agent: config.agent_index,
    });

    for step in 1..=SWARM_MAX_STEPS {
        if config.cancel.load(Ordering::Relaxed) {
            return finish_aborted(config.agent_index, &config.events, outcome);
        }
        outcome.steps = step;
        let _ = config.events.send(SwarmUiEvent::AgentStep {
            agent: config.agent_index,
            step,
        });
        let _ = config.events.send(SwarmUiEvent::AgentStatus {
            agent: config.agent_index,
            line: "Thinking".to_owned(),
            detail: None,
        });

        let step_result = provider.complete_agent_step(
            &config.model_id,
            &history,
            &tools,
            &config.request_options,
        );

        match step_result {
            Ok(AgentStep {
                assistant_message,
                result: AgentStepResult::Text(content),
                usage,
            }) => {
                history.push(assistant_message);
                send_usage(&config, usage.as_ref());
                outcome.completed = true;
                outcome.summary = truncate_summary(&guard.redact(&content));
                let _ = config.events.send(SwarmUiEvent::AgentFinished {
                    agent: config.agent_index,
                    summary: outcome.summary.clone(),
                });
                return outcome;
            }
            Ok(AgentStep {
                assistant_message,
                result: AgentStepResult::ToolCalls(calls),
                usage,
            }) => {
                history.push(assistant_message);
                send_usage(&config, usage.as_ref());

                for call in &calls {
                    if config.cancel.load(Ordering::Relaxed) {
                        return finish_aborted(config.agent_index, &config.events, outcome);
                    }
                    let action = describe_tool_call(&call.name, &call.arguments);
                    let _ = config.events.send(SwarmUiEvent::AgentStatus {
                        agent: config.agent_index,
                        line: action,
                        detail: tool_call_detail(&call.arguments),
                    });

                    let result = match policy.evaluate(&working_dir, &call.name, &call.arguments)
                    {
                        PolicyDecision::Deny { reason } => format!(
                            "error: blocked by RaptorCode policy ({reason}). Choose a safer approach and continue."
                        ),
                        PolicyDecision::Confirm(request) => {
                            let (reply_tx, reply_rx) = channel::<bool>();
                            let _ = config.events.send(SwarmUiEvent::AgentNeedsConfirmation {
                                agent: config.agent_index,
                                message: request.message.clone(),
                                reply: reply_tx,
                            });
                            if reply_rx.recv().unwrap_or(false) {
                                if request.kind == ConfirmationKind::OutsideWorkspaceWrite {
                                    policy.allow_outside_writes_for_session();
                                }
                                execute_tool_call(&mut working_dir, &call.name, &call.arguments)
                            } else {
                                format!(
                                    "error: the developer declined this action ({}). Ask for guidance or choose a different approach.",
                                    request.message
                                )
                            }
                        }
                        PolicyDecision::Allow => {
                            execute_tool_call(&mut working_dir, &call.name, &call.arguments)
                        }
                    };

                    if call.name == "write_file" && !result.starts_with("error:") {
                        if let Some(path) = tool_call_detail(&call.arguments) {
                            outcome.files_written += 1;
                            let _ = config.events.send(SwarmUiEvent::AgentFileWritten {
                                agent: config.agent_index,
                                path,
                            });
                        }
                    }

                    history.push(AgentMessage {
                        role: "tool".to_owned(),
                        content: Some(result),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                    });
                }

                context_budget::agent_notices(&mut history, usage.as_ref(), &mut budget);
            }
            Err(error) => {
                let message = format!("agent error: {error}");
                let _ = config.events.send(SwarmUiEvent::AgentFailed {
                    agent: config.agent_index,
                    error: message.clone(),
                });
                outcome.summary = message;
                return outcome;
            }
        }
    }

    outcome.completed = true;
    outcome.summary =
        format!("step limit ({SWARM_MAX_STEPS}) reached; partial work saved in the worktree");
    let _ = config.events.send(SwarmUiEvent::AgentFinished {
        agent: config.agent_index,
        summary: outcome.summary.clone(),
    });
    outcome
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

fn send_usage(config: &SwarmWorkerConfig, usage: Option<&raptor_providers::TokenUsage>) {
    if let Some(total) = usage.and_then(|usage| usage.total_tokens) {
        let _ = config.events.send(SwarmUiEvent::AgentUsage {
            agent: config.agent_index,
            total_tokens: u64::from(total),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_system_prompts_carry_shared_base_and_specialization() {
        for role in [
            "frontend-agent",
            "backend-agent",
            "test-agent",
            "security-agent",
        ] {
            let prompt = swarm_role_system_prompt(role);
            assert!(prompt.contains("isolated git worktree"), "{role}");
            assert!(prompt.contains("SOLID"), "{role}");
        }
        assert!(swarm_role_system_prompt("frontend-agent").contains("frontend engineering"));
        assert!(swarm_role_system_prompt("security-agent").contains("SECURITY_NOTES.md"));
    }

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
}
