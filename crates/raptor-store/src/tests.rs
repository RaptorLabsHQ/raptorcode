//! Store tests: every case runs in its own temp directory and exercises the
//! public API plus the crate-private atomic write and TOML escaper.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use raptor_agent::{AgentId, AgentRole, AgentSession};
use raptor_core::{SwarmStatus, TaskId, TaskMetadata, TaskStatus, WorktreeInfo};

use super::*;
use crate::atomic_io::atomic_write;
use crate::toml_codec::escape_toml_string;

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
            "raptor-store-{name}-{}-{nonce}",
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
fn init_creates_config_when_missing() {
    let test_dir = TestDir::create("create");
    let outcome = init_project_config(&test_dir.path).unwrap();
    let config_path = test_dir.path.join(CONFIG_FILE_NAME);

    assert_eq!(outcome, InitConfigOutcome::Created(config_path.clone()));
    let content = fs::read_to_string(config_path).unwrap();

    assert!(content.contains("version = 1"));
    assert!(content.contains("[providers.deepseek]"));
    assert!(content.contains("enabled = true"));
}

#[test]
fn init_refuses_to_overwrite_existing_config() {
    let test_dir = TestDir::create("exists");
    let config_path = test_dir.path.join(CONFIG_FILE_NAME);
    fs::write(&config_path, "custom = true\n").unwrap();

    let outcome = init_project_config(&test_dir.path).unwrap();

    assert_eq!(outcome, InitConfigOutcome::Exists(config_path.clone()));
    assert_eq!(fs::read_to_string(config_path).unwrap(), "custom = true\n");
}

#[test]
fn init_rejects_parent_dir_components() {
    let error = init_project_config(Path::new("../unsafe")).unwrap_err();

    assert!(matches!(error, InitConfigError::UnsafeBasePath(_)));
}

#[test]
fn reads_provider_config_from_project_config() {
    let test_dir = TestDir::create("provider-config");
    fs::write(
        test_dir.path.join(CONFIG_FILE_NAME),
        "version = 1\n\n[providers.deepseek]\nenabled = true\n\n[providers.openai]\nenabled = false\n",
    )
    .unwrap();

    let config = read_project_config(&test_dir.path).unwrap();

    assert_eq!(config.version, Some(1));
    assert_eq!(config.providers.len(), 2);
    assert_eq!(config.providers[0].id, "deepseek");
    assert!(config.providers[0].enabled);
    assert_eq!(config.providers[1].id, "openai");
    assert!(!config.providers[1].enabled);
}

#[test]
fn provider_config_handles_empty_provider_sections() {
    let test_dir = TestDir::create("provider-config-empty");
    fs::write(test_dir.path.join(CONFIG_FILE_NAME), "version = 1\n").unwrap();

    let config = read_project_config(&test_dir.path).unwrap();

    assert!(config.providers.is_empty());
}

#[test]
fn provider_config_rejects_unsafe_provider_ids() {
    let test_dir = TestDir::create("provider-config-unsafe");
    fs::write(
        test_dir.path.join(CONFIG_FILE_NAME),
        "[providers.DeepSeek]\nenabled = true\n",
    )
    .unwrap();

    let error = read_project_config(&test_dir.path).unwrap_err();

    assert!(matches!(error, ProjectConfigError::UnsafeProviderId(_)));
}

#[test]
fn missing_project_config_is_explicit() {
    let test_dir = TestDir::create("provider-config-missing");

    let error = read_project_config(&test_dir.path).unwrap_err();

    assert!(matches!(error, ProjectConfigError::NotFound(_)));
}

#[test]
fn persists_agent_session_without_overwrite() {
    let test_dir = TestDir::create("agent-persist");
    let session =
        AgentSession::new_read_only(AgentId::new("agent-worker-123").unwrap(), AgentRole::Worker);

    let metadata_path = persist_agent_session(&test_dir.path, &session).unwrap();

    assert!(metadata_path.exists());
    assert!(matches!(
        persist_agent_session(&test_dir.path, &session).unwrap_err(),
        AgentStoreError::AlreadyExists(_)
    ));
}

#[test]
fn list_returns_empty_when_no_agents_exist() {
    let test_dir = TestDir::create("agent-list-empty");

    assert!(list_agent_sessions(&test_dir.path).unwrap().is_empty());
}

#[test]
fn list_reads_one_agent_session() {
    let test_dir = TestDir::create("agent-list-one");
    let mut session = AgentSession::new_read_only(
        AgentId::new("agent-reviewer-123").unwrap(),
        AgentRole::Reviewer,
    );
    session.set_provider_id("deepseek").unwrap();
    persist_agent_session(&test_dir.path, &session).unwrap();

    let sessions = list_agent_sessions(&test_dir.path).unwrap();

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session.id.as_str(), "agent-reviewer-123");
    assert_eq!(sessions[0].session.role, AgentRole::Reviewer);
    assert_eq!(sessions[0].session.provider_id.as_deref(), Some("deepseek"));
    assert!(sessions[0].session.created_at.is_some());
    assert!(sessions[0].session.updated_at.is_some());
}

#[test]
fn show_reads_existing_agent_session() {
    let test_dir = TestDir::create("agent-show");
    let mut session =
        AgentSession::new_read_only(AgentId::new("agent-1").unwrap(), AgentRole::Architect);
    session.set_model_id("deepseek-v4-pro").unwrap();
    persist_agent_session(&test_dir.path, &session).unwrap();

    let stored = read_agent_session(&test_dir.path, "agent-1").unwrap();

    assert_eq!(stored.session.id.as_str(), "agent-1");
    assert_eq!(stored.session.role, AgentRole::Architect);
    assert_eq!(stored.session.model_id.as_deref(), Some("deepseek-v4-pro"));
}

#[test]
fn show_missing_agent_returns_not_found() {
    let test_dir = TestDir::create("agent-missing");

    let error = read_agent_session(&test_dir.path, "missing-agent").unwrap_err();

    assert!(matches!(error, AgentStoreError::NotFound(_)));
}

#[test]
fn rejects_unsafe_agent_id() {
    let test_dir = TestDir::create("agent-unsafe");

    let error = read_agent_session(&test_dir.path, "../agent").unwrap_err();

    assert!(matches!(error, AgentStoreError::UnsafeAgentId(_)));
}

#[test]
fn persists_task_metadata_without_overwrite() {
    let test_dir = TestDir::create("task");
    let task_id = TaskId::from_description("Persist task metadata").unwrap();
    let metadata = TaskMetadata::new(
        task_id.clone(),
        "Persist task metadata",
        TaskStatus::Planned,
        WorktreeInfo {
            branch_name: format!("raptor-task-{}", task_id.as_str()),
            path: test_dir
                .path
                .join(".raptor")
                .join("worktrees")
                .join(task_id.as_str()),
            repository_root: test_dir.path.clone(),
        },
    );

    let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();

    assert!(metadata_path.exists());
    assert!(task_metadata_exists(&test_dir.path, &task_id).unwrap());
    assert!(matches!(
        persist_task_metadata(&test_dir.path, &metadata).unwrap_err(),
        TaskStoreError::AlreadyExists(_)
    ));
}

#[test]
fn atomic_write_replaces_existing_content_without_leftovers() {
    let test_dir = TestDir::create("atomic-write");
    let path = test_dir.path.join("data.txt");

    atomic_write(&path, "first").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "first");

    atomic_write(&path, "second-and-longer").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "second-and-longer");

    let leftover_temp = fs::read_dir(&test_dir.path)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().contains(".tmp."));
    assert!(!leftover_temp, "atomic_write left a temp file behind");
}

#[test]
fn list_task_metadata_skips_torn_files() {
    let test_dir = TestDir::create("task-list-torn");
    let task_id = TaskId::from_description("List one healthy task").unwrap();
    let metadata = TaskMetadata::new(
        task_id.clone(),
        "List one healthy task",
        TaskStatus::Planned,
        WorktreeInfo {
            branch_name: format!("raptor-task-{}", task_id.as_str()),
            path: test_dir
                .path
                .join(".raptor")
                .join("worktrees")
                .join(task_id.as_str()),
            repository_root: test_dir.path.clone(),
        },
    );
    persist_task_metadata(&test_dir.path, &metadata).unwrap();

    // A torn file with a valid-looking id but unparseable content must not
    // break listing of the healthy task.
    let torn_path = test_dir
        .path
        .join(RAPTOR_STATE_DIR)
        .join(TASKS_DIR)
        .join("corrupt-task-deadbeef.toml");
    fs::write(&torn_path, "this line has no equals sign\n").unwrap();
    assert!(read_task_metadata(&test_dir.path, "corrupt-task-deadbeef").is_err());

    let listed = list_task_metadata(&test_dir.path).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id.as_str(), task_id.as_str());
}

#[test]
fn persists_swarm_metadata_without_overwrite() {
    let test_dir = TestDir::create("swarm");
    let task_id = TaskId::from_description("Swarm metadata").unwrap();
    let metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm metadata");

    let metadata_path = persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
    let content = fs::read_to_string(&metadata_path).unwrap();

    assert!(metadata_path.exists());
    assert!(swarm_metadata_exists(&test_dir.path, &task_id).unwrap());
    assert!(content.contains(&format!("id = \"{}\"", task_id.as_str())));
    assert!(content.contains("provider = \"deepseek\""));
    assert!(content.contains("model = \"deepseek-v4-pro\""));
    assert!(content.contains("[[agents]]"));
    assert!(content.contains("role = \"frontend-agent\""));
    assert!(matches!(
        persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err(),
        SwarmStoreError::AlreadyExists(_)
    ));
}

#[test]
fn list_reads_persisted_swarm_metadata() {
    let test_dir = TestDir::create("swarm-list");
    let task_id = TaskId::from_description("Swarm list").unwrap();
    let metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm list");

    persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
    fs::write(
        test_dir
            .path
            .join(".raptor")
            .join("swarms")
            .join("ignore.txt"),
        "ignore",
    )
    .unwrap();

    let swarms = list_swarm_metadata(&test_dir.path).unwrap();

    assert_eq!(swarms.len(), 1);
    assert_eq!(swarms[0].id, task_id.as_str());
    assert_eq!(swarms[0].agents.len(), 1);
}

#[test]
fn updates_swarm_planning_metadata() {
    let test_dir = TestDir::create("swarm-update");
    let task_id = TaskId::from_description("Swarm update").unwrap();
    let mut metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Swarm update");
    let metadata_path = persist_swarm_metadata(&test_dir.path, &metadata).unwrap();
    let plan_file = metadata.agents[0].worktree_path.join("RAPTOR_PLAN.md");
    let proposal_file = metadata.agents[0]
        .worktree_path
        .join("RAPTOR_IMPLEMENTATION_PROPOSAL.md");

    metadata.status = SwarmStatus::PlanningCompleted;
    metadata.planning_status = Some(SwarmStatus::PlanningCompleted);
    metadata.agents[0].status = SwarmStatus::PlanningCompleted;
    metadata.agents[0].implementation_status = Some(SwarmStatus::ProposalGenerated);
    metadata.agents[0].planning_status = Some(SwarmStatus::PlanningCompleted);
    metadata.agents[0].plan_file_path = Some(plan_file.clone());
    metadata.agents[0].proposal_file_path = Some(proposal_file.clone());
    metadata.agents[0].provider = Some("deepseek".to_owned());
    metadata.agents[0].model = Some("deepseek-v4-pro".to_owned());

    let updated_path = update_swarm_metadata(&test_dir.path, &metadata).unwrap();
    let content = fs::read_to_string(&updated_path).unwrap();

    assert_eq!(updated_path, metadata_path);
    assert!(content.contains("planning_status = \"planning_completed\""));
    // Mirror the metadata renderer's TOML escaping so the substring matches
    // on Windows, where the path's backslashes are stored doubled.
    assert!(content.contains(&format!(
        "plan_file_path = \"{}\"",
        escape_toml_string(&plan_file.display().to_string())
    )));
    assert!(content.contains("implementation_status = \"proposal_generated\""));
    assert!(content.contains(&format!(
        "proposal_file_path = \"{}\"",
        escape_toml_string(&proposal_file.display().to_string())
    )));
    assert!(content.contains("provider = \"deepseek\""));
    assert!(content.contains("model = \"deepseek-v4-pro\""));
}

#[test]
fn reads_swarm_metadata_with_planning_and_proposal_fields() {
    let test_dir = TestDir::create("swarm-read");
    let task_id = TaskId::from_description("Read swarm").unwrap();
    let mut metadata = test_swarm_metadata(&test_dir.path, task_id.clone(), "Read swarm");
    let plan_file = metadata.agents[0].worktree_path.join("RAPTOR_PLAN.md");
    let proposal_file = metadata.agents[0]
        .worktree_path
        .join("RAPTOR_IMPLEMENTATION_PROPOSAL.md");

    metadata.planning_status = Some(SwarmStatus::PlanningCompleted);
    metadata.agents[0].implementation_status = Some(SwarmStatus::ProposalGenerated);
    metadata.agents[0].planning_status = Some(SwarmStatus::PlanningCompleted);
    metadata.agents[0].plan_file_path = Some(plan_file.clone());
    metadata.agents[0].proposal_file_path = Some(proposal_file.clone());
    metadata.agents[0].provider = Some("deepseek".to_owned());
    metadata.agents[0].model = Some("deepseek-v4-pro".to_owned());
    persist_swarm_metadata(&test_dir.path, &metadata).unwrap();

    let stored = read_swarm_metadata(&test_dir.path, task_id.as_str()).unwrap();

    assert_eq!(stored.id, task_id.as_str());
    assert_eq!(stored.description, "Read swarm");
    assert_eq!(stored.planning_status, Some(SwarmStatus::PlanningCompleted));
    assert_eq!(stored.agents.len(), 1);
    assert_eq!(
        stored.agents[0].implementation_status,
        Some(SwarmStatus::ProposalGenerated)
    );
    assert_eq!(stored.agents[0].plan_file_path.as_ref(), Some(&plan_file));
    assert_eq!(
        stored.agents[0].proposal_file_path.as_ref(),
        Some(&proposal_file)
    );
}

#[test]
fn swarm_metadata_rejects_unsafe_values() {
    let test_dir = TestDir::create("swarm-unsafe");
    let task_id = TaskId::from_description("Unsafe swarm metadata").unwrap();
    let mut metadata =
        test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");

    metadata.agents[0].worktree_path = test_dir.path.join("..").join("outside");
    let path_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
    assert!(matches!(path_error, SwarmStoreError::UnsafeBasePath(_)));

    let mut metadata =
        test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
    metadata.agents[0].branch = "../main".to_owned();
    let branch_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
    assert!(matches!(branch_error, SwarmStoreError::UnsafeValue { .. }));

    let mut metadata =
        test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
    metadata.agents[0].plan_file_path = Some(test_dir.path.join("outside-plan.md"));
    let plan_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
    assert!(matches!(plan_error, SwarmStoreError::UnsafeBasePath(_)));

    let mut metadata =
        test_swarm_metadata(&test_dir.path, task_id.clone(), "Unsafe swarm metadata");
    metadata.agents[0].proposal_file_path = Some(test_dir.path.join("outside-proposal.md"));
    let proposal_error = persist_swarm_metadata(&test_dir.path, &metadata).unwrap_err();
    assert!(matches!(proposal_error, SwarmStoreError::UnsafeBasePath(_)));
}

#[test]
fn list_returns_empty_when_no_tasks_exist() {
    let test_dir = TestDir::create("list-empty");

    assert!(list_task_metadata(&test_dir.path).unwrap().is_empty());
}

#[test]
fn list_reads_one_persisted_task() {
    let test_dir = TestDir::create("list-one");
    let task_id = TaskId::from_description("List one task").unwrap();
    let metadata = test_metadata(&test_dir.path, task_id.clone(), "List one task");
    persist_task_metadata(&test_dir.path, &metadata).unwrap();

    let tasks = list_task_metadata(&test_dir.path).unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task_id.as_str());
    assert_eq!(tasks[0].status, Some(TaskStatus::Planned));
    assert_eq!(tasks[0].description.as_deref(), Some("List one task"));
    assert!(tasks[0].created_at.is_some());
    assert!(tasks[0].updated_at.is_some());
}

#[test]
fn show_reads_existing_task() {
    let test_dir = TestDir::create("show");
    let task_id = TaskId::from_description("Show task").unwrap();
    let metadata = test_metadata(&test_dir.path, task_id.clone(), "Show task");
    persist_task_metadata(&test_dir.path, &metadata).unwrap();

    let task = read_task_metadata(&test_dir.path, task_id.as_str()).unwrap();

    assert_eq!(task.id, task_id.as_str());
    assert_eq!(task.description.as_deref(), Some("Show task"));
}

#[test]
fn show_missing_task_returns_not_found() {
    let test_dir = TestDir::create("missing");

    let error = read_task_metadata(&test_dir.path, "missing-task").unwrap_err();

    assert!(matches!(error, TaskStoreError::NotFound(_)));
}

#[test]
fn rejects_unsafe_task_id() {
    let test_dir = TestDir::create("unsafe");

    let error = read_task_metadata(&test_dir.path, "Unsafe").unwrap_err();

    assert!(matches!(error, TaskStoreError::UnsafeTaskId(_)));
}

#[test]
fn rejects_path_traversal_task_id() {
    let test_dir = TestDir::create("traversal");

    let error = read_task_metadata(&test_dir.path, "../secret").unwrap_err();

    assert!(matches!(error, TaskStoreError::UnsafeTaskId(_)));
}

#[test]
fn task_reads_preserve_metadata_contents() {
    let test_dir = TestDir::create("readonly");
    let task_id = TaskId::from_description("Read only").unwrap();
    let metadata = test_metadata(&test_dir.path, task_id.clone(), "Read only");
    let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();
    let before = fs::read_to_string(&metadata_path).unwrap();
    let before_entries = fs::read_dir(test_dir.path.join(RAPTOR_STATE_DIR).join(TASKS_DIR))
        .unwrap()
        .count();

    list_task_metadata(&test_dir.path).unwrap();
    read_task_metadata(&test_dir.path, task_id.as_str()).unwrap();

    assert_eq!(fs::read_to_string(metadata_path).unwrap(), before);
    assert_eq!(
        fs::read_dir(test_dir.path.join(RAPTOR_STATE_DIR).join(TASKS_DIR))
            .unwrap()
            .count(),
        before_entries
    );
}

#[test]
fn updates_task_status_with_transition_timestamp() {
    let test_dir = TestDir::create("status-update");
    let task_id = TaskId::from_description("Update status").unwrap();
    let metadata = test_metadata(&test_dir.path, task_id.clone(), "Update status");
    persist_task_metadata(&test_dir.path, &metadata).unwrap();

    let updated = update_task_status(
        &test_dir.path,
        task_id.as_str(),
        TaskStatusTransition::Merged,
    )
    .unwrap();

    assert_eq!(updated.status, Some(TaskStatus::Merged));
    assert!(updated.updated_at.is_some());
    assert!(updated.merged_at.is_some());
    assert_eq!(updated.conflict_at, None);
    assert_eq!(updated.abandoned_at, None);
}

#[test]
fn marks_task_abandoned_without_deleting_metadata() {
    let test_dir = TestDir::create("status-abandoned");
    let task_id = TaskId::from_description("Abandon status").unwrap();
    let metadata = test_metadata(&test_dir.path, task_id.clone(), "Abandon status");
    let metadata_path = persist_task_metadata(&test_dir.path, &metadata).unwrap();

    let updated = update_task_status(
        &test_dir.path,
        task_id.as_str(),
        TaskStatusTransition::Abandoned,
    )
    .unwrap();

    assert_eq!(updated.status, Some(TaskStatus::Abandoned));
    assert!(updated.abandoned_at.is_some());
    assert!(metadata_path.exists());
}

#[test]
fn escapes_task_metadata_strings() {
    assert_eq!(
        escape_toml_string("quote \" slash \\ newline\n"),
        "quote \\\" slash \\\\ newline\\n"
    );
}

fn test_metadata(repository_root: &Path, task_id: TaskId, description: &str) -> TaskMetadata {
    TaskMetadata::new(
        task_id.clone(),
        description,
        TaskStatus::Planned,
        WorktreeInfo {
            branch_name: format!("raptor-task-{}", task_id.as_str()),
            path: repository_root
                .join(RAPTOR_STATE_DIR)
                .join("worktrees")
                .join(task_id.as_str()),
            repository_root: repository_root.to_path_buf(),
        },
    )
}

fn test_swarm_metadata(
    repository_root: &Path,
    task_id: TaskId,
    description: &str,
) -> SwarmMetadata {
    let role = "frontend-agent";

    SwarmMetadata {
        agents: vec![SwarmAgentMetadata {
            agent_id: format!("{}-{role}", task_id.as_str()),
            branch: format!("raptor/{}/{role}", task_id.as_str()),
            implementation_status: None,
            model: None,
            plan_file_path: None,
            planning_status: None,
            proposal_file_path: None,
            provider: None,
            role: role.to_owned(),
            status: SwarmStatus::PendingModelExecution,
            worktree_path: repository_root
                .join(RAPTOR_STATE_DIR)
                .join("worktrees")
                .join(task_id.as_str())
                .join(role),
        }],
        created_at: None,
        description: description.to_owned(),
        id: task_id.as_str().to_owned(),
        model: "deepseek-v4-pro".to_owned(),
        planning_status: None,
        provider: "deepseek".to_owned(),
        status: SwarmStatus::PendingModelExecution,
        updated_at: None,
    }
}
