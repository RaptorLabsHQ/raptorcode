//! Interactive REPL slash-command parsing and dispatch.
//!
//! This module owns everything that happens when a developer types a `/command`
//! (or `?`) at the interactive prompt: classifying the line into a typed
//! [`SlashCommand`] and executing it against the live session state. Keeping it
//! out of the main REPL loop in `lib.rs` makes the command set explicit and the
//! classification unit-testable. The handlers perform terminal I/O and mutate
//! session state through [`SlashSession`]; the loop in `lib.rs` only constructs
//! the session and acts on the returned [`ReplControl`].

use std::io::Write as _;
use std::path::Path;

use raptor_providers::{
    CompletionMessage, DeepSeekProvider, DeepSeekReasoningEffort, DeepSeekRequestOptions,
    DeepSeekResponseFormat, DeepSeekThinkingMode, ModelId,
};

use crate::context_budget::ContextBudgetState;
use crate::{
    agent_system_message, chat_system_prompt, format_model_display_name, interactive_select,
    prompt_provider_and_api_key, render_interactive_help, test_deepseek_connection,
    write_stored_deepseek_api_key, AgentMessage, MenuItem, C_BOLD, C_CYAN, C_DIM, C_GREEN_BOLD,
    C_RED, C_RED_BOLD, C_RESET,
};

/// A recognized interactive slash command. `parse` returns `None` for any line
/// that is not a command, so the REPL treats it as a normal prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SlashCommand {
    /// `/swarm <mission>` — the mission is trimmed; empty means "show usage".
    Swarm(String),
    Exit,
    Help,
    Clear,
    Thinking,
    Effort,
    Stream,
    Json,
    Balance,
    Api,
    Model,
}

/// What the REPL loop should do after a slash command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplControl {
    /// Command handled — continue the loop and read the next line.
    Continue,
    /// Exit the interactive session.
    Break,
}

/// Live REPL state a slash command may read or mutate. Borrowed fresh each
/// iteration so handlers can reassign the provider, key, model, and toggles in
/// place and reset conversation history when a setting changes.
pub(crate) struct SlashSession<'a> {
    pub current_dir: &'a Path,
    pub agent_mode: bool,
    pub provider: &'a mut DeepSeekProvider,
    pub api_key: &'a mut String,
    pub model_id: &'a mut ModelId,
    pub request_options: &'a mut DeepSeekRequestOptions,
    pub streaming_enabled: &'a mut bool,
    pub json_mode: &'a mut bool,
    pub agent_history: &'a mut Vec<AgentMessage>,
    pub chat_history: &'a mut Vec<CompletionMessage>,
    pub context_budget: &'a mut ContextBudgetState,
}

impl SlashSession<'_> {
    /// Rebuilds chat and agent history from fresh system messages and clears the
    /// context budget. Used whenever a setting change must reset the conversation
    /// (model, thinking mode, effort, JSON mode, `/clear`).
    fn reset_conversation(&mut self) {
        *self.agent_history = vec![agent_system_message(self.current_dir, *self.json_mode)];
        *self.chat_history = vec![CompletionMessage::system(chat_system_prompt(
            *self.json_mode,
        ))];
        *self.context_budget = ContextBudgetState::default();
    }
}

/// Classifies an input line into a [`SlashCommand`], or `None` when it is not a
/// command and should be handled as a normal prompt.
///
/// `/swarm` is matched case-sensitively on its prefix (so `/swarmfoo` and
/// `/Swarm` fall through to a normal prompt, preserving prior behavior); the
/// remaining commands match case-insensitively on the whole line.
pub(crate) fn parse(input: &str) -> Option<SlashCommand> {
    if let Some(rest) = input.strip_prefix("/swarm") {
        if rest.is_empty() || rest.starts_with(' ') {
            return Some(SlashCommand::Swarm(rest.trim().to_owned()));
        }
    }

    match input.to_lowercase().as_str() {
        "/exit" | "/quit" => Some(SlashCommand::Exit),
        "/help" | "?" => Some(SlashCommand::Help),
        "/clear" => Some(SlashCommand::Clear),
        "/thinking" => Some(SlashCommand::Thinking),
        "/effort" => Some(SlashCommand::Effort),
        "/stream" => Some(SlashCommand::Stream),
        "/json" => Some(SlashCommand::Json),
        "/balance" => Some(SlashCommand::Balance),
        "/api" => Some(SlashCommand::Api),
        "/model" => Some(SlashCommand::Model),
        _ => None,
    }
}

/// Executes a parsed slash command against the live session, returning whether
/// the REPL should continue or exit.
pub(crate) fn dispatch(command: SlashCommand, session: &mut SlashSession) -> ReplControl {
    match command {
        SlashCommand::Swarm(mission) => handle_swarm(&mission, session),
        SlashCommand::Exit => {
            println!("Goodbye.");
            ReplControl::Break
        }
        SlashCommand::Help => {
            print!("{}", render_interactive_help());
            ReplControl::Continue
        }
        SlashCommand::Clear => {
            session.reset_conversation();
            println!("Conversation cleared.\n");
            ReplControl::Continue
        }
        SlashCommand::Thinking => handle_thinking(session),
        SlashCommand::Effort => handle_effort(session),
        SlashCommand::Stream => handle_stream(session),
        SlashCommand::Json => handle_json(session),
        SlashCommand::Balance => handle_balance(session),
        SlashCommand::Api => handle_api(session),
        SlashCommand::Model => handle_model(session),
    }
}

fn handle_swarm(mission: &str, session: &mut SlashSession) -> ReplControl {
    if mission.is_empty() {
        println!("  usage: /swarm <mission description>\n");
        return ReplControl::Continue;
    }
    if !session.agent_mode {
        println!("  {C_RED}✗ /swarm needs agentic mode.{C_RESET}  {C_DIM}Restart raptor and grant file system access.{C_RESET}\n");
        return ReplControl::Continue;
    }
    println!();
    let result = crate::swarm_orchestrator::run_live_swarm(
        session.current_dir,
        crate::swarm_orchestrator::LiveSwarmConfig {
            api_key: session.api_key.clone(),
            mission: mission.to_owned(),
            model_label: format_model_display_name(session.model_id.as_str()),
            model_id: session.model_id.clone(),
            // Swarm agents always run ultrathink with text output,
            // independent of the session /json or /effort settings.
            request_options: DeepSeekRequestOptions::default()
                .with_reasoning_effort(DeepSeekReasoningEffort::Max),
        },
    );
    match result {
        Ok(summary) => println!("{summary}"),
        Err(error) => println!("  {C_RED}✗ swarm failed:{C_RESET} {error}\n"),
    }
    ReplControl::Continue
}

fn handle_thinking(session: &mut SlashSession) -> ReplControl {
    session.request_options.thinking_mode = match session.request_options.thinking_mode {
        DeepSeekThinkingMode::Enabled => DeepSeekThinkingMode::Disabled,
        DeepSeekThinkingMode::Disabled => DeepSeekThinkingMode::Enabled,
    };
    let label = session.request_options.thinking_mode.label();
    session.reset_conversation();
    println!(
        "  {C_GREEN_BOLD}✓  DeepSeek {label}{C_RESET}  {C_DIM}(conversation reset){C_RESET}\n"
    );
    ReplControl::Continue
}

fn handle_effort(session: &mut SlashSession) -> ReplControl {
    session.request_options.reasoning_effort = match session.request_options.reasoning_effort {
        DeepSeekReasoningEffort::High => DeepSeekReasoningEffort::Max,
        DeepSeekReasoningEffort::Max => DeepSeekReasoningEffort::High,
    };
    let label = session.request_options.reasoning_effort.label();
    session.reset_conversation();
    println!(
        "  {C_GREEN_BOLD}✓  DeepSeek {label}{C_RESET}  {C_DIM}(conversation reset){C_RESET}\n"
    );
    ReplControl::Continue
}

fn handle_stream(session: &mut SlashSession) -> ReplControl {
    *session.streaming_enabled = !*session.streaming_enabled;
    println!(
        "  {C_GREEN_BOLD}✓  Streaming {}{C_RESET}\n",
        if *session.streaming_enabled {
            "on"
        } else {
            "off"
        }
    );
    ReplControl::Continue
}

fn handle_json(session: &mut SlashSession) -> ReplControl {
    *session.json_mode = !*session.json_mode;
    session.request_options.response_format = if *session.json_mode {
        DeepSeekResponseFormat::JsonObject
    } else {
        DeepSeekResponseFormat::Text
    };
    session.reset_conversation();
    println!(
        "  {C_GREEN_BOLD}✓  JSON output {}{C_RESET}  {C_DIM}(conversation reset){C_RESET}\n",
        if *session.json_mode { "on" } else { "off" }
    );
    ReplControl::Continue
}

fn handle_balance(session: &mut SlashSession) -> ReplControl {
    println!();
    print!("  {C_DIM}Fetching DeepSeek balance...{C_RESET}");
    std::io::stdout().flush().ok();
    match session.provider.get_user_balance() {
        Ok(balance) => {
            println!(" done\r");
            println!(
                "  {C_BOLD}DeepSeek balance{C_RESET}  {}",
                if balance.is_available {
                    format!("{C_GREEN_BOLD}available{C_RESET}")
                } else {
                    format!("{C_RED_BOLD}unavailable{C_RESET}")
                }
            );
            if balance.balance_infos.is_empty() {
                println!("  {C_DIM}No balance rows returned.{C_RESET}");
            }
            for info in balance.balance_infos {
                println!(
                    "  {C_DIM}{}:{C_RESET} total={} granted={} topped_up={}",
                    info.currency, info.total_balance, info.granted_balance, info.topped_up_balance
                );
            }
        }
        Err(error) => {
            println!();
            eprintln!("  {C_RED}✗  {error}{C_RESET}");
        }
    }
    println!();
    ReplControl::Continue
}

fn handle_api(session: &mut SlashSession) -> ReplControl {
    println!();
    println!("  {C_BOLD}API Key Settings{C_RESET}  {C_DIM}↑/↓ · Enter{C_RESET}");
    println!();
    let api_items = vec![
        MenuItem {
            label: "Change API Key".to_owned(),
            sublabel: None,
            enabled: true,
        },
        MenuItem {
            label: "Show current API Key".to_owned(),
            sublabel: None,
            enabled: true,
        },
        MenuItem {
            label: "Cancel".to_owned(),
            sublabel: None,
            enabled: true,
        },
    ];
    match interactive_select(&api_items) {
        Some(0) => match prompt_provider_and_api_key() {
            Ok(new_key) => {
                print!("\n  {C_DIM}⟳  Testing connection...{C_RESET}");
                std::io::stdout().flush().ok();
                match DeepSeekProvider::new(&new_key) {
                    Ok(new_prov) => match test_deepseek_connection(&new_prov) {
                        Ok(()) => match write_stored_deepseek_api_key(&new_key) {
                            Ok(()) => {
                                *session.provider = new_prov;
                                *session.api_key = new_key;
                                println!(
                                    "\n  {C_GREEN_BOLD}✓  API key updated and saved.{C_RESET}"
                                );
                            }
                            Err(e) => eprintln!("\n  failed to save key: {e}"),
                        },
                        Err(e) => eprintln!("\n  {C_RED}✗  {e}{C_RESET}"),
                    },
                    Err(e) => eprintln!("\n  {C_RED}✗  {e}{C_RESET}"),
                }
            }
            Err(e) => eprintln!("  {e}"),
        },
        Some(1) => {
            let masked = if session.api_key.len() > 12 {
                format!(
                    "{}...{}",
                    &session.api_key[..8],
                    &session.api_key[session.api_key.len() - 4..]
                )
            } else {
                "***".to_owned()
            };
            println!("  {C_DIM}Current API key:{C_RESET} {C_CYAN}{masked}{C_RESET}");
        }
        _ => {}
    }
    println!();
    ReplControl::Continue
}

fn handle_model(session: &mut SlashSession) -> ReplControl {
    println!();
    print!("  {C_DIM}Fetching available models...{C_RESET}");
    std::io::stdout().flush().ok();
    let model_list = match session.provider.list_models() {
        Ok(models) if !models.is_empty() => {
            println!(" done\r");
            models
        }
        _ => {
            println!(" using defaults\r");
            vec!["deepseek-v4-pro".to_owned(), "deepseek-v4-flash".to_owned()]
        }
    };
    println!();
    println!("  {C_BOLD}Select model{C_RESET}  {C_DIM}↑/↓ · Enter{C_RESET}");
    println!();
    let model_items: Vec<MenuItem> = model_list
        .iter()
        .map(|m| MenuItem {
            label: m.clone(),
            sublabel: if m.as_str() == session.model_id.as_str() {
                Some("current".to_owned())
            } else {
                None
            },
            enabled: true,
        })
        .collect();
    if let Some(idx) = interactive_select(&model_items) {
        if let Ok(new_model) = ModelId::new(&model_list[idx]) {
            *session.model_id = new_model;
            session.reset_conversation();
            println!(
                "  {C_GREEN_BOLD}✓  Model: {}{C_RESET}  {C_DIM}(conversation reset){C_RESET}",
                session.model_id.as_str()
            );
        }
    }
    println!();
    ReplControl::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swarm_with_mission_and_trims() {
        assert_eq!(
            parse("/swarm build a checkout flow"),
            Some(SlashCommand::Swarm("build a checkout flow".to_owned()))
        );
        assert_eq!(
            parse("/swarm   spaced   "),
            Some(SlashCommand::Swarm("spaced".to_owned()))
        );
    }

    #[test]
    fn parses_bare_swarm_as_empty_mission() {
        assert_eq!(parse("/swarm"), Some(SlashCommand::Swarm(String::new())));
        assert_eq!(parse("/swarm   "), Some(SlashCommand::Swarm(String::new())));
    }

    #[test]
    fn swarm_prefix_is_case_sensitive_and_word_bounded() {
        // No space after the command, and wrong case, both fall through to a
        // normal prompt rather than being treated as /swarm.
        assert_eq!(parse("/swarmfoo"), None);
        assert_eq!(parse("/Swarm mission"), None);
        assert_eq!(parse("/SWARM"), None);
    }

    #[test]
    fn parses_case_insensitive_simple_commands() {
        assert_eq!(parse("/exit"), Some(SlashCommand::Exit));
        assert_eq!(parse("/QUIT"), Some(SlashCommand::Exit));
        assert_eq!(parse("/Help"), Some(SlashCommand::Help));
        assert_eq!(parse("?"), Some(SlashCommand::Help));
        assert_eq!(parse("/clear"), Some(SlashCommand::Clear));
        assert_eq!(parse("/thinking"), Some(SlashCommand::Thinking));
        assert_eq!(parse("/effort"), Some(SlashCommand::Effort));
        assert_eq!(parse("/stream"), Some(SlashCommand::Stream));
        assert_eq!(parse("/json"), Some(SlashCommand::Json));
        assert_eq!(parse("/balance"), Some(SlashCommand::Balance));
        assert_eq!(parse("/api"), Some(SlashCommand::Api));
        assert_eq!(parse("/MODEL"), Some(SlashCommand::Model));
    }

    #[test]
    fn non_commands_return_none() {
        assert_eq!(parse("write a function"), None);
        assert_eq!(parse("/unknown"), None);
        assert_eq!(parse("/exit now"), None); // exact match only
        assert_eq!(parse(""), None);
    }
}
