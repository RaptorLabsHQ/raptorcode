use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDiagnostic {
    pub available: bool,
    pub detail: String,
    pub name: String,
}

pub fn command_version(command: &str, args: &[&str]) -> ToolDiagnostic {
    match Command::new(command).args(args).output() {
        Ok(output) if output.status.success() => ToolDiagnostic {
            available: true,
            detail: first_non_empty_line(&output.stdout)
                .or_else(|| first_non_empty_line(&output.stderr))
                .unwrap_or_else(|| "version output unavailable".to_owned()),
            name: command.to_owned(),
        },
        Ok(output) => ToolDiagnostic {
            available: false,
            detail: format!("command exited with {}", output.status),
            name: command.to_owned(),
        },
        Err(error) => ToolDiagnostic {
            available: false,
            detail: error.to_string(),
            name: command.to_owned(),
        },
    }
}

fn first_non_empty_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_first_non_empty_line() {
        assert_eq!(
            first_non_empty_line(b"\n\ncargo 1.0\n"),
            Some("cargo 1.0".to_owned())
        );
    }

    #[test]
    fn reports_missing_command_as_unavailable() {
        let diagnostic = command_version("raptor-code-missing-command", &["--version"]);

        assert!(!diagnostic.available);
        assert_eq!(diagnostic.name, "raptor-code-missing-command");
    }
}
