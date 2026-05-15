pub const PROJECT_DESCRIPTION: &str = "An open-source Rust-native multi-agent coding terminal that runs AI coding agents safely in isolated git worktrees with developer supervision.";
pub const PROJECT_NAME: &str = "RaptorCode";
pub const V1_SCOPE: &str = "CLI + TUI + multi-agent orchestrator";

pub fn project_description() -> &'static str {
    PROJECT_DESCRIPTION
}

pub fn project_name() -> &'static str {
    PROJECT_NAME
}

pub fn v1_scope() -> &'static str {
    V1_SCOPE
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_project_metadata() {
        assert_eq!(project_name(), "RaptorCode");
        assert!(project_description().contains("multi-agent coding terminal"));
        assert_eq!(v1_scope(), "CLI + TUI + multi-agent orchestrator");
    }
}
