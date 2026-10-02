//! Task and swarm command implementations.
//!
//! This module owns the `raptor task` / `raptor swarm` lifecycle command
//! handlers — creation, planning, implementation, review, diff, test, and merge —
//! together with their presentation (`render_*`) helpers. It was extracted from
//! the large `lib.rs` REPL/dispatch core so that command logic lives behind one
//! cohesive boundary. Generic display utilities (number/path formatting, diff
//! section builders) live in `render` and are shared via `pub(crate)`.

use std::fs;
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};

use raptor_agent::{build_role_planning_prompt, role_prompt_template};
use raptor_core::{SwarmStatus, TaskId, TaskMetadata, TaskStatus};
use raptor_git::{
    create_swarm_worktrees, create_task_worktree, diff_swarm_role_worktree, diff_task_worktree,
    execute_task_merge, execute_task_worktree_cleanup, inspect_repository_root,
    inspect_swarm_role_worktree, inspect_task_worktree, plan_task_worktree,
    plan_task_worktree_cleanup, preflight_task_merge, validated_swarm_role_worktree_path,
    validated_task_worktree_path, GitError, MergeExecution, MergePreflight, RepositoryRoot,
    SwarmWorktreePlan, WorktreeCleanupPlan, WorktreeDiff, WorktreeReview,
};
use raptor_providers::{
    CompletionMessage, CompletionRequest, CompletionResponse, DeepSeekProvider,
    DeepSeekReasoningEffort, DeepSeekRequestOptions, Provider, ProviderError, DEEPSEEK_API_KEY_ENV,
    DEEPSEEK_API_KEY_URL,
};
use raptor_store::{
    list_task_metadata, persist_swarm_metadata, persist_task_metadata, read_swarm_metadata,
    read_task_metadata, swarm_metadata_exists, task_metadata_exists, update_swarm_metadata,
    update_task_status, StoredTaskMetadata, SwarmAgentMetadata, SwarmMetadata,
    TaskStatusTransition, TaskStoreError,
};
use raptor_tools::{
    detect_test_command, explicit_test_command, run_test_command, TestCommandOutput,
};

use crate::prompt_guard::PromptGuard;
use crate::render::{
    display_exit_code, display_optional, display_path_optional, push_changed_files_section,
    push_command_output_section, push_diff_section, strip_line_ending, task_status_allows_cleanup,
};
use crate::repl::resolve_deepseek_api_key;
use crate::{
    ci_environment_enabled, format_model_display_name, prompts, swarm_orchestrator, CliError,
    RAPTOR_IMPLEMENTATION_PROPOSAL_FILE_NAME, RAPTOR_PLAN_FILE_NAME, SWARM_MODEL_ID,
    SWARM_PROVIDER_DISPLAY, SWARM_PROVIDER_ID,
};

pub fn create_task(current_dir: &Path, description: &str) -> Result<String, CliError> {
    let trimmed_description = description.trim();
    let task_id = TaskId::from_description(trimmed_description)?;
    let repository_root = inspect_repository_root(current_dir)?;

    if task_metadata_exists(repository_root.path(), &task_id)? {
        return Err(CliError::new(format!(
            "task metadata already exists for {}; refusing to overwrite",
            task_id.as_str()
        )));
    }

    let plan = plan_task_worktree(&repository_root, &task_id)?;
    let worktree = create_task_worktree(&repository_root, &task_id)?;
    let metadata = TaskMetadata::new(
        task_id.clone(),
        trimmed_description,
        TaskStatus::Planned,
        worktree,
    );
    let metadata_path = persist_task_metadata(repository_root.path(), &metadata)?;

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
pub(crate) enum SwarmPlanningSource<'a> {
    Environment,
    MissingDeepSeekApiKey,
    Provider(&'a dyn Provider),
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum SwarmImplementationSource<'a> {
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

pub(crate) fn create_swarm_with_planning_source(
    current_dir: &Path,
    description: &str,
    planning_source: SwarmPlanningSource<'_>,
) -> Result<String, CliError> {
    let trimmed_description = description.trim();
    let task_id = TaskId::from_description(trimmed_description)?;
    let repository_root = inspect_repository_root(current_dir)?;

    if task_metadata_exists(repository_root.path(), &task_id)? {
        return Err(CliError::new(format!(
            "task metadata already exists for {}; refusing to overwrite",
            task_id.as_str()
        )));
    }

    if swarm_metadata_exists(repository_root.path(), &task_id)? {
        return Err(CliError::new(format!(
            "swarm metadata already exists for {}; refusing to overwrite",
            task_id.as_str()
        )));
    }

    let plan = create_swarm_worktrees(&repository_root, &task_id)?;
    let mut metadata = swarm_metadata_from_plan(&plan, trimmed_description);
    let metadata_path = persist_swarm_metadata(repository_root.path(), &metadata)?;

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

    update_swarm_metadata(repository_root.path(), &metadata)?;

    Ok(render_swarm_created(&metadata, &metadata_path))
}

pub(crate) fn cmd_swarm_run(current_dir: &Path, description: &str) -> Result<String, CliError> {
    use std::io::IsTerminal as _;

    if description.trim().is_empty() {
        return Err(CliError::new("swarm run mission description is required"));
    }
    if !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
        || ci_environment_enabled()
    {
        return Err(CliError::new(
            "swarm run requires an interactive terminal for the live dashboard",
        ));
    }
    let api_key = resolve_deepseek_api_key().ok_or_else(|| {
        CliError::new(format!(
            "DeepSeek API key required. Set {DEEPSEEK_API_KEY_ENV} or run `raptor` once to save a key. Get one: {DEEPSEEK_API_KEY_URL}"
        ))
    })?;
    let provider = DeepSeekProvider::new(&api_key)?;
    let model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model"))?;

    swarm_orchestrator::run_live_swarm(
        current_dir,
        swarm_orchestrator::LiveSwarmConfig {
            api_key,
            mission: description.trim().to_owned(),
            model_label: format_model_display_name(model_id.as_str()),
            model_id,
            // Ultrathink defaults; swarm agents always use text output.
            request_options: DeepSeekRequestOptions::default()
                .with_reasoning_effort(DeepSeekReasoningEffort::Max),
        },
    )
}

pub(crate) fn swarm_metadata_from_plan(
    plan: &SwarmWorktreePlan,
    description: &str,
) -> SwarmMetadata {
    let agents = plan
        .agents
        .iter()
        .map(|agent| SwarmAgentMetadata {
            agent_id: format!("{}-{}", plan.task_id, agent.role),
            branch: agent.branch_name.clone(),
            implementation_status: None,
            model: Some(SWARM_MODEL_ID.to_owned()),
            plan_file_path: None,
            planning_status: Some(SwarmStatus::PendingModelExecution),
            proposal_file_path: None,
            provider: Some(SWARM_PROVIDER_ID.to_owned()),
            role: agent.role.clone(),
            status: SwarmStatus::PendingModelExecution,
            worktree_path: agent.path.clone(),
        })
        .collect();

    SwarmMetadata {
        agents,
        created_at: None,
        description: description.to_owned(),
        id: plan.task_id.clone(),
        model: SWARM_MODEL_ID.to_owned(),
        planning_status: Some(SwarmStatus::PendingModelExecution),
        provider: SWARM_PROVIDER_ID.to_owned(),
        status: SwarmStatus::PendingModelExecution,
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
    let guard = PromptGuard::from_prompts();
    let mut failed_roles = Vec::new();

    for agent in &mut metadata.agents {
        let prompt = build_role_planning_prompt(task_id.as_str(), task_description, &agent.role)?;
        let request = CompletionRequest::new(
            provider.id().clone(),
            model_id.clone(),
            vec![
                CompletionMessage::system(prompts::swarm_planning_system_prompt()),
                CompletionMessage::user(prompt),
            ],
        );

        match provider.complete(request) {
            Ok(mut response) => {
                response.content = guard.redact(&response.content);
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
                    agent.status = SwarmStatus::PlanningFailed;
                    agent.planning_status = Some(SwarmStatus::PlanningFailed);
                    agent.plan_file_path = None;
                    failed_roles.push(format!("{}: {}", agent.role, error));
                    continue;
                }

                agent.status = SwarmStatus::PlanningCompleted;
                agent.planning_status = Some(SwarmStatus::PlanningCompleted);
                agent.plan_file_path = Some(plan_file_path);
                agent.provider = Some(response.provider_id.as_str().to_owned());
                agent.model = Some(response.model_id.as_str().to_owned());
            }
            Err(error) => {
                agent.status = SwarmStatus::PlanningFailed;
                agent.planning_status = Some(SwarmStatus::PlanningFailed);
                agent.plan_file_path = None;
                failed_roles.push(format!("{}: {}", agent.role, error));
            }
        }
    }

    if failed_roles.is_empty() {
        metadata.status = SwarmStatus::PlanningCompleted;
        metadata.planning_status = Some(SwarmStatus::PlanningCompleted);
    } else {
        metadata.status = SwarmStatus::PlanningFailed;
        metadata.planning_status = Some(SwarmStatus::PlanningFailed);
    }

    Ok(())
}

fn mark_swarm_planning_missing_api_key(metadata: &mut SwarmMetadata) {
    metadata.status = SwarmStatus::BlockedMissingDeepseekApiKey;
    metadata.planning_status = Some(SwarmStatus::BlockedMissingDeepseekApiKey);

    for agent in &mut metadata.agents {
        agent.status = SwarmStatus::BlockedMissingDeepseekApiKey;
        agent.planning_status = Some(SwarmStatus::BlockedMissingDeepseekApiKey);
        agent.plan_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.model = Some(SWARM_MODEL_ID.to_owned());
    }
}

fn mark_swarm_planning_failed(metadata: &mut SwarmMetadata, _detail: Option<String>) {
    metadata.status = SwarmStatus::PlanningFailed;
    metadata.planning_status = Some(SwarmStatus::PlanningFailed);

    for agent in &mut metadata.agents {
        agent.status = SwarmStatus::PlanningFailed;
        agent.planning_status = Some(SwarmStatus::PlanningFailed);
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

pub(crate) fn implement_swarm_role_with_source(
    current_dir: &Path,
    task_id: &str,
    role: &str,
    implementation_source: SwarmImplementationSource<'_>,
) -> Result<String, CliError> {
    role_prompt_template(role)?;

    let repository_root = inspect_repository_root(current_dir)?;
    let mut metadata = read_swarm_metadata(repository_root.path(), task_id)?;
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

    if agent.planning_status == Some(SwarmStatus::PlanningCompleted) && !plan_file_path.is_file() {
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
            CompletionMessage::system(prompts::swarm_implementation_system_prompt()),
            CompletionMessage::user(prompt),
        ],
    );
    let mut response = match provider.complete(request) {
        Ok(response) => response,
        Err(error) => {
            mark_swarm_role_implementation_failed(repository_root, metadata, agent_index)?;
            return Err(CliError::new(error.to_string()));
        }
    };
    response.content = PromptGuard::from_prompts().redact(&response.content);
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
        agent.implementation_status = Some(SwarmStatus::ProposalGenerated);
        agent.model = Some(response.model_id.as_str().to_owned());
        agent.proposal_file_path = Some(proposal_file_path.clone());
        agent.provider = Some(response.provider_id.as_str().to_owned());
        agent.status = SwarmStatus::ProposalGenerated;
    }

    update_swarm_metadata(repository_root, metadata)?;

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
        agent.implementation_status = Some(SwarmStatus::BlockedMissingDeepseekApiKey);
        agent.model = Some(SWARM_MODEL_ID.to_owned());
        agent.proposal_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.status = SwarmStatus::BlockedMissingDeepseekApiKey;
    }

    update_swarm_metadata(repository_root, metadata)?;

    Ok(())
}

fn mark_swarm_role_implementation_failed(
    repository_root: &Path,
    metadata: &mut SwarmMetadata,
    agent_index: usize,
) -> Result<(), CliError> {
    {
        let agent = &mut metadata.agents[agent_index];
        agent.implementation_status = Some(SwarmStatus::Failed);
        agent.model = Some(SWARM_MODEL_ID.to_owned());
        agent.proposal_file_path = None;
        agent.provider = Some(SWARM_PROVIDER_ID.to_owned());
        agent.status = SwarmStatus::Failed;
    }

    update_swarm_metadata(repository_root, metadata)?;

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
            .implementation_status.unwrap_or(agent.status).as_str(),
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
            .implementation_status.unwrap_or(agent.status).as_str()
    )
}

pub fn list_tasks(current_dir: &Path) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let tasks = list_task_metadata(repository_root.path())?;

    if tasks.is_empty() {
        return Ok("No RaptorCode tasks found.\n".to_owned());
    }

    let mut output = String::from("ID | Status | Created | Worktree | Description\n");
    output.push_str("--- | --- | --- | --- | ---\n");

    for task in tasks {
        output.push_str(&format!(
            "{} | {} | {} | {} | {}\n",
            task.id,
            display_optional(task.status.map(TaskStatus::as_str)),
            display_optional(task.created_at.as_deref()),
            display_path_optional(task.worktree_path.as_ref()),
            display_optional(task.description.as_deref())
        ));
    }

    Ok(output)
}

pub fn show_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let task = read_task_metadata(repository_root.path(), task_id)?;

    Ok(render_task_details(&task))
}

pub fn abandon_task(current_dir: &Path, task_id: &str) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let task = read_task_metadata(repository_root.path(), task_id)?;
    let status = task.status.map(TaskStatus::as_str).unwrap_or("unknown");

    if task.status == Some(TaskStatus::Merged) {
        return Err(CliError::new(format!(
            "task status {status} cannot be abandoned"
        )));
    }

    let updated = update_task_status(
        repository_root.path(),
        &task.id,
        TaskStatusTransition::Abandoned,
    )?;

    Ok(format!(
        "Task abandoned\nTask: {}\nStatus: {}\nWorktree retained: {}\nMetadata: {}\n",
        updated.id,
        display_optional(updated.status.map(TaskStatus::as_str)),
        display_path_optional(updated.worktree_path.as_ref()),
        updated.metadata_path.display()
    ))
}

pub fn cleanup_task(
    current_dir: &Path,
    task_id: &str,
    confirmation_input: Option<&mut dyn BufRead>,
) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let task = read_task_metadata(repository_root.path(), task_id)?;
    let status = task.status.map(TaskStatus::as_str).unwrap_or("unknown");
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let plan = plan_task_worktree_cleanup(&repository_root, &task.id, worktree_path)?;

    if !task_status_allows_cleanup(task.status) {
        return Err(CliError::new(format!(
            "task status {status} is not safe for cleanup; expected merged or abandoned"
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
    let repository_root = inspect_repository_root(current_dir)?;
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
    let review = inspect_task_worktree(&repository_root, &task.id, worktree_path)?;

    Ok(render_task_review(&task, &review))
}

pub fn diff_task(
    current_dir: &Path,
    task_id: &str,
    role: Option<&str>,
) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
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
    let diff = diff_task_worktree(&repository_root, &task.id, worktree_path)?;

    Ok(render_task_diff(&task, &diff))
}

pub fn test_task(
    current_dir: &Path,
    task_id: &str,
    explicit_command_parts: Option<&[String]>,
) -> Result<String, CliError> {
    let repository_root = inspect_repository_root(current_dir)?;
    let explicit_command = explicit_command_parts
        .map(explicit_test_command)
        .transpose()?;
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
        let output = run_test_command(&target.worktree_path, &command)?;

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
                validated_task_worktree_path(repository_root, &task.id, worktree_path)?;

            Ok((
                "single-task",
                vec![TestTarget {
                    label: task.id,
                    worktree_path,
                }],
            ))
        }
        Err(TaskStoreError::NotFound(_)) => {
            let metadata = read_swarm_metadata(repository_root.path(), task_id)?;
            let mut targets = Vec::new();

            for agent in &metadata.agents {
                role_prompt_template(&agent.role)?;
                let worktree_path = validated_swarm_role_worktree_path(
                    repository_root,
                    &metadata.id,
                    &agent.role,
                    &agent.worktree_path,
                )?;
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
        role_prompt_template(role)?;
    }

    let metadata = read_swarm_metadata(repository_root.path(), task_id)?;
    let agents = selected_swarm_agents(&metadata, role)?;
    let mut reviews = Vec::new();

    for agent in agents {
        let review = inspect_swarm_role_worktree(
            repository_root,
            &metadata.id,
            &agent.role,
            &agent.worktree_path,
        )?;
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
        role_prompt_template(role)?;
    }

    let metadata = read_swarm_metadata(repository_root.path(), task_id)?;
    let agents = selected_swarm_agents(&metadata, role)?;
    let mut diffs = Vec::new();

    for agent in agents {
        let diff = diff_swarm_role_worktree(
            repository_root,
            &metadata.id,
            &agent.role,
            &agent.worktree_path,
        )?;
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

    role_prompt_template(role)?;
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
    let repository_root = inspect_repository_root(current_dir)?;
    let task = read_task_metadata(repository_root.path(), task_id)?;
    let worktree_path = task
        .worktree_path
        .as_ref()
        .ok_or_else(|| CliError::new("task metadata is missing worktree_path"))?;
    let preflight = preflight_task_merge(&repository_root, &task.id, worktree_path)?;

    Ok((repository_root, task, preflight))
}

fn render_swarm_created(metadata: &SwarmMetadata, metadata_path: &Path) -> String {
    let mut output = format!(
        "RaptorCode Swarm Created\nTask: {}\nDescription: {}\nProvider: {}\nModel: {}\nStatus: {}\nPlanning status: {}\nMetadata: {}\n",
        metadata.id,
        metadata.description,
        SWARM_PROVIDER_DISPLAY,
        metadata.model,
        metadata.status,
        metadata.planning_status.map_or("-", SwarmStatus::as_str),
        metadata_path.display()
    );

    output.push_str("Agents:\n");
    for agent in &metadata.agents {
        output.push_str(&format!(
            "  {} | branch: {} | worktree: {} | plan: {}",
            agent.role,
            agent.branch,
            agent.worktree_path.display(),
            agent.planning_status.unwrap_or(agent.status).as_str()
        ));

        if let Some(plan_file_path) = agent.plan_file_path.as_ref() {
            output.push_str(&format!(" | plan file: {}", plan_file_path.display()));
        }

        output.push('\n');
    }

    if metadata.planning_status == Some(SwarmStatus::BlockedMissingDeepseekApiKey) {
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
        display_optional(task.status.map(TaskStatus::as_str)),
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
        display_optional(task.status.map(TaskStatus::as_str)),
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
        metadata.planning_status.map_or("-", SwarmStatus::as_str),
        role.unwrap_or("all"),
        reviews.len()
    );

    for item in reviews {
        output.push_str(&format!(
            "\nAgent: {}\nStatus: {}\nPlanning status: {}\nImplementation status: {}\nWorktree: {}\nPlan file: {}\nProposal file: {}\n",
            item.agent.role,
            item.agent.status,
            item.agent.planning_status.map_or("-", SwarmStatus::as_str),
            item.agent.implementation_status.map_or("-", SwarmStatus::as_str),
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
        display_optional(task.status.map(TaskStatus::as_str)),
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
        display_optional(task.status.map(TaskStatus::as_str)),
        display_optional(task.updated_at.as_deref()),
        task.metadata_path.display()
    )
}
