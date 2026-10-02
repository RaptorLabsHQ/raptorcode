//! Local project state under `.raptor/` — tasks, swarms, agent sessions, and
//! config — persisted as a small hand-rolled TOML subset with atomic writes.
//!
//! Each store lives in its own module; this file holds the shared constants and
//! re-exports every public item so callers keep importing `raptor_store::<name>`
//! unqualified.

use std::time::{SystemTime, UNIX_EPOCH};

mod agent;
mod atomic_io;
mod config;
mod paths;
mod swarm;
mod task;
mod toml_codec;

#[cfg(test)]
mod tests;

pub use agent::{
    list_agent_sessions, persist_agent_session, read_agent_session, AgentStoreError,
    StoredAgentSession,
};
pub use config::{
    init_project_config, read_project_config, InitConfigError, InitConfigOutcome, ProjectConfig,
    ProjectConfigError, ProjectProviderConfig,
};
pub use swarm::{
    list_swarm_metadata, persist_swarm_metadata, read_swarm_metadata, swarm_metadata_exists,
    swarm_metadata_path, update_swarm_metadata, SwarmAgentMetadata, SwarmMetadata, SwarmStoreError,
};
pub use task::{
    list_task_metadata, persist_task_metadata, read_task_metadata, task_metadata_exists,
    task_metadata_path, update_task_status, StoredTaskMetadata, TaskStatusTransition,
    TaskStoreError,
};

pub const AGENTS_DIR: &str = "agents";
pub const CONFIG_FILE_NAME: &str = "raptor.toml";
pub const CRATE_PURPOSE: &str = "Local state storage boundaries for RaptorCode.";
pub const RAPTOR_STATE_DIR: &str = ".raptor";
pub const SWARMS_DIR: &str = "swarms";
pub const TASKS_DIR: &str = "tasks";

/// Seconds since the Unix epoch as a decimal string (`"0"` if the clock is
/// before the epoch); every store stamps `created_at` / `updated_at` with it.
pub(crate) fn current_unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}
