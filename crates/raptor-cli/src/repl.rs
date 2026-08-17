//! The interactive `raptor` terminal: the read-eval-print loop the user lands in
//! when running `raptor` with no arguments. Owns the session loop
//! (`run_interactive_chat`), the tool-executing single-agent loop
//! (`run_agent_loop`), the prompt input box, the thinking spinner and prompt
//! quote ticker, local credential storage/consent, and the embedded quote pool.
//! Slash commands are delegated to `slash_command`; the default swarm run and
//! tool execution to `swarm_orchestrator`/`agent_tools`.
//!
//! Like the other extracted modules, a single `use crate::*` re-imports the
//! parent's private aliases instead of duplicating `lib.rs`'s import list.
use crate::*;

pub fn run_interactive_chat(
    current_dir: &Path,
    startup_options: &StartupBannerOptions,
) -> Result<(), CliError> {
    use std::io::Write as _;

    // Resolve latest DeepSeek model for the banner if we have a saved key
    let mut banner_options = startup_options.clone();
    if let Some(saved_key) = resolve_deepseek_api_key() {
        if let Ok(temp_provider) = DeepSeekProvider::new(saved_key) {
            let (tx, rx) = std::sync::mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(temp_provider.list_models());
            });
            if let Ok(Ok(models)) = rx.recv_timeout(Duration::from_secs(3)) {
                if let Some(best) = pick_latest_model(&models) {
                    banner_options.model_name = Some(format_model_display_name(&best));
                }
            }
        }
    }
    print!("{}", render_startup_banner(&banner_options));
    std::io::stdout().flush().ok();

    let (mut api_key, is_new_key) = match resolve_deepseek_api_key() {
        Some(key) => (key, false),
        None => {
            let key = prompt_provider_and_api_key()?;
            (key, true)
        }
    };

    let mut provider =
        DeepSeekProvider::new(&api_key).map_err(|error| CliError::new(error.to_string()))?;

    if is_new_key {
        println!("\n  {C_DIM}⟳  Testing connection to DeepSeek API...{C_RESET}");
        std::io::stdout().flush().ok();
        test_deepseek_connection(&provider)?;
        write_stored_deepseek_api_key(&api_key)?;
        println!("  {C_GREEN_BOLD}✓  Connection established.{C_RESET}{C_DIM}  API key saved to ~/.raptor/.credentials{C_RESET}");
        println!("  {C_GREEN_BOLD}✓  Ready to code.{C_RESET}");
    }

    let agent_mode = prompt_file_access_consent(current_dir);

    if agent_mode {
        println!("  {C_GREEN_BOLD}✓  Agentic mode enabled.{C_RESET}  {C_DIM}Full file system and shell access granted.{C_RESET}");
    } else {
        println!("  {C_DIM}Chat mode active.{C_RESET}  No files will be modified.");
    }

    let mut model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model"))?;
    // Ultrathink defaults: thinking mode enabled (provider default) and maximum
    // reasoning effort from session start in both chat and agentic mode.
    // /thinking and /effort remain available to back off.
    let mut request_options =
        DeepSeekRequestOptions::default().with_reasoning_effort(DeepSeekReasoningEffort::Max);
    let mut streaming_enabled = true;
    let mut json_mode = false;

    println!();
    println!("{C_DIM}─────────────────────────────────────────────────────{C_RESET}");
    let workflow_label = format!(
        "{} · Agentic Coding · {} · {} · {} output",
        format_model_display_name(model_id.as_str()),
        request_options.thinking_mode.label(),
        request_options.reasoning_effort.label(),
        request_options.response_format.label()
    );
    println!(
        "  {C_RED_BOLD}RaptorCode Engineering Agent{C_RESET}  {C_DIM}│{C_RESET}  {workflow_label}"
    );
    println!("  Type your request and press Enter to send.");
    println!("  {C_DIM}/help · /swarm · /api · /model · /thinking · /effort · /stream · /json · /balance · Ctrl+D or /exit{C_RESET}");
    println!(
        "  {C_DIM}Streaming: {}{C_RESET}",
        if streaming_enabled { "on" } else { "off" }
    );
    println!("{C_DIM}─────────────────────────────────────────────────────{C_RESET}\n");

    let tools = build_agent_tools();
    let mut agent_working_dir = current_dir.to_path_buf();
    let mut context_budget = ContextBudgetState::default();
    let prompt_guard = PromptGuard::from_prompts();
    let mut agent_tool_policy = ToolPolicy::new(
        current_dir
            .canonicalize()
            .unwrap_or_else(|_| current_dir.to_path_buf()),
    );

    let mut agent_history: Vec<AgentMessage> = vec![agent_system_message(current_dir, json_mode)];
    let mut chat_history: Vec<CompletionMessage> =
        vec![CompletionMessage::system(chat_system_prompt(json_mode))];

    loop {
        let input = match read_boxed_agent_prompt_line(banner_options.color_mode) {
            Ok(None) => {
                println!("\nGoodbye.");
                break;
            }
            Ok(Some(input)) => input,
            Err(error) => {
                eprintln!("input error: {error}");
                break;
            }
        };

        if input.is_empty() {
            continue;
        }

        if let Some(command) = slash_command::parse(&input) {
            let mut session = slash_command::SlashSession {
                current_dir,
                agent_mode,
                provider: &mut provider,
                api_key: &mut api_key,
                model_id: &mut model_id,
                request_options: &mut request_options,
                streaming_enabled: &mut streaming_enabled,
                json_mode: &mut json_mode,
                agent_history: &mut agent_history,
                chat_history: &mut chat_history,
                context_budget: &mut context_budget,
            };
            match slash_command::dispatch(command, &mut session) {
                slash_command::ReplControl::Continue => continue,
                slash_command::ReplControl::Break => break,
            }
        }

        println!();

        if agent_mode {
            // Default: every prompt launches the live 4-agent swarm with the
            // dashboard. The single-agent loop below is only a fallback for when
            // the swarm cannot run here (for example, outside a git repository).
            let swarm_result = swarm_orchestrator::run_live_swarm(
                current_dir,
                swarm_orchestrator::LiveSwarmConfig {
                    api_key: api_key.clone(),
                    mission: input.clone(),
                    model_label: format_model_display_name(model_id.as_str()),
                    model_id: model_id.clone(),
                    // Swarm agents always run ultrathink with text output,
                    // independent of the session /json or /effort settings.
                    request_options: DeepSeekRequestOptions::default()
                        .with_reasoning_effort(DeepSeekReasoningEffort::Max),
                },
            );
            let run_single_agent = match swarm_result {
                Ok(summary) => {
                    print!("{summary}");
                    std::io::stdout().flush().ok();
                    false
                }
                Err(error) => {
                    println!(
                        "  {C_DIM}Swarm unavailable here ({error}). Answering with a single agent.{C_RESET}\n"
                    );
                    true
                }
            };
            if run_single_agent {
                agent_history.push(AgentMessage {
                    role: "user".to_owned(),
                    content: Some(input.clone()),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                });

                let mut agent_loop_context = AgentLoopContext {
                    context_budget: &mut context_budget,
                    model_id: &model_id,
                    prompt_guard: &prompt_guard,
                    provider: &provider,
                    request_options: &request_options,
                    streaming_enabled,
                    tool_policy: &mut agent_tool_policy,
                    tools: &tools,
                };
                match run_agent_loop(
                    &mut agent_working_dir,
                    &mut agent_history,
                    &mut agent_loop_context,
                ) {
                    Ok(output) => {
                        let content = output.content.trim().to_owned();
                        if output.streamed {
                            println!("\n");
                        } else {
                            println!("{}\n", prompt_guard.redact(&content));
                        }
                        if let Some(usage) = output.usage.as_ref() {
                            let line = render_context_usage(usage);
                            if !line.is_empty() {
                                println!("  {C_DIM}{line}{C_RESET}\n");
                            }
                        }
                        let assistant_message =
                            output.assistant_message.unwrap_or_else(|| AgentMessage {
                                role: "assistant".to_owned(),
                                content: Some(content),
                                reasoning_content: None,
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        agent_history.push(assistant_message);
                        handle_agent_context_budget(
                            &mut agent_history,
                            output.usage.as_ref(),
                            &mut context_budget,
                        );
                    }
                    Err(error) => {
                        let msg = error.to_string();
                        eprintln!("Error: {msg}\n");
                        if msg.contains("authentication") {
                            eprintln!(
                                "Delete ~/.raptor/.credentials or set DEEPSEEK_API_KEY to reset."
                            );
                            break;
                        }
                    }
                }
            }
        } else {
            chat_history.push(CompletionMessage::user(&input));

            let request = CompletionRequest::new(
                provider.id().clone(),
                model_id.clone(),
                chat_history.clone(),
            )
            .with_options(request_options.clone());

            let response = if streaming_enabled {
                print!("  {C_DIM}Streaming agent response...{C_RESET}\n\n");
                std::io::stdout().flush().ok();
                let mut redactor = StreamRedactor::new(&prompt_guard);
                let response = provider.complete_streaming(request, |delta| {
                    if delta.reasoning_content.is_some() {
                        return;
                    }
                    if let Some(content) = delta.content {
                        print!("{}", redactor.push(&content));
                        std::io::stdout().flush().ok();
                    }
                });
                print!("{}", redactor.finish());
                std::io::stdout().flush().ok();
                response
            } else {
                provider.complete(request)
            };

            match response {
                Ok(response) => {
                    let content = response.content.trim().to_owned();
                    if streaming_enabled {
                        println!("\n");
                    } else {
                        println!("{}\n", prompt_guard.redact(&content));
                    }
                    if let Some(usage) = response.usage.as_ref() {
                        let line = render_context_usage(usage);
                        if !line.is_empty() {
                            println!("  {C_DIM}{line}{C_RESET}\n");
                        }
                    }
                    chat_history.push(CompletionMessage::assistant(&content));
                    handle_chat_context_budget(
                        &mut chat_history,
                        response.usage.as_ref(),
                        &mut context_budget,
                    );
                }
                Err(ProviderError::MissingApiKey) | Err(ProviderError::AuthenticationFailed(_)) => {
                    chat_history.pop();
                    eprintln!("Authentication failed. Your API key may be invalid.");
                    eprintln!("Delete ~/.raptor/.credentials or set DEEPSEEK_API_KEY to reset.\n");
                    break;
                }
                Err(error) => {
                    chat_history.pop();
                    eprintln!("Error: {error}\n");
                }
            }
        }
    }

    Ok(())
}

fn read_boxed_agent_prompt_line(color_mode: ColorMode) -> std::io::Result<Option<String>> {
    use crossterm::{cursor, execute};

    const AGENT_INPUT_LABEL: &str = ">  ";

    let width = terminal_width_from_environment();
    let mut stdout = std::io::stdout();
    print!(
        "{}",
        render_prompt_input_box(width, color_mode, AGENT_INPUT_LABEL)
    );
    // Reserve a line beneath the box for the rotating quote hint.
    println!();
    stdout.flush()?;
    // Move the cursor up onto the input line — past the reserved quote line and
    // the box's bottom border — and into the input column.
    execute!(
        stdout,
        cursor::MoveUp(2),
        cursor::MoveToColumn(prompt_input_cursor_column(AGENT_INPUT_LABEL))
    )?;
    stdout.flush()?;

    // Rotate saved quotes on the reserved line while the developer types and
    // submits the prompt. Purely decorative; it never touches stdin.
    let ticker = PromptQuoteTicker::start(width);

    // Preferred path: a raw-mode line editor that owns the echo, so input can
    // never run through the box's right border or wrap onto the border/quote
    // lines below — long input scrolls horizontally inside the field instead.
    if let Some(read) = read_boxed_line_raw(
        prompt_input_cursor_column(AGENT_INPUT_LABEL),
        prompt_input_field_width(width, AGENT_INPUT_LABEL),
    ) {
        ticker.stop();
        // The raw editor leaves the cursor on the input line: step past the
        // bottom border onto the (now stale) quote line, clear it, and leave the
        // cursor there so conversation output starts on a clean line below.
        execute!(
            stdout,
            cursor::MoveDown(2),
            cursor::MoveToColumn(0),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
        )
        .ok();
        return read.map(|line| line.map(|text| text.trim().to_owned()));
    }

    // Fallback (raw mode unavailable, e.g. stdin is not a real terminal): the
    // terminal's own cooked-mode line editing. Overlong input can wrap here,
    // but this path only runs where the box is not interactively usable anyway.
    let mut input = String::new();
    let read = std::io::stdin().read_line(&mut input);
    ticker.stop();

    // The terminal echoed the submit newline, dropping the cursor onto the box's
    // bottom border. Step onto the (now stale) quote line, clear it, and leave the
    // cursor there so conversation output starts on a clean line below the box.
    execute!(
        stdout,
        cursor::MoveDown(1),
        cursor::MoveToColumn(0),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
    )
    .ok();

    match read {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(input.trim().to_owned())),
        Err(error) => Err(error),
    }
}

/// Restores cooked mode when dropped, so an early return, error, or panic can
/// never strand the terminal in raw mode.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Option<Self> {
        crossterm::terminal::enable_raw_mode().ok()?;
        Some(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Raw-mode line editor for the boxed prompt. Reads key events directly and
/// re-renders only the editable field between the label and the right border,
/// so the box stays intact no matter how long the input grows. Returns `None`
/// when raw mode cannot be enabled so the caller can fall back to a cooked
/// read, `Some(Ok(None))` on Ctrl+C / Ctrl+D-at-empty (end the session), and
/// `Some(Ok(Some(text)))` on Enter.
fn read_boxed_line_raw(
    origin_col: u16,
    field_width: usize,
) -> Option<std::io::Result<Option<String>>> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    if field_width < 2 {
        return None;
    }
    let guard = RawModeGuard::enable()?;

    let mut editor = BoxedLineEditor::new();
    let outcome = loop {
        let key = match event::read() {
            Ok(Event::Key(key)) => key,
            Ok(_) => continue,
            Err(error) => break Err(error),
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let plain = !ctrl && !key.modifiers.contains(KeyModifiers::ALT);
        let changed = match key.code {
            KeyCode::Enter => break Ok(Some(editor.text())),
            // Match the cooked-mode expectations: Ctrl+D on an empty line is
            // EOF; Ctrl+C ends the session the same graceful way.
            KeyCode::Char('c') if ctrl => break Ok(None),
            KeyCode::Char('d') if ctrl => {
                if editor.is_empty() {
                    break Ok(None);
                }
                editor.delete_forward()
            }
            KeyCode::Char('u') if ctrl => editor.kill_to_start(),
            KeyCode::Char('k') if ctrl => editor.kill_to_end(),
            KeyCode::Char('w') if ctrl => editor.delete_word_back(),
            KeyCode::Char('a') if ctrl => editor.move_home(),
            KeyCode::Char('e') if ctrl => editor.move_end(),
            KeyCode::Char(ch) if plain => {
                editor.insert(ch);
                true
            }
            KeyCode::Backspace => editor.backspace(),
            KeyCode::Delete => editor.delete_forward(),
            KeyCode::Left => editor.move_left(),
            KeyCode::Right => editor.move_right(),
            KeyCode::Home => editor.move_home(),
            KeyCode::End => editor.move_end(),
            // A literal tab would break the column math; insert a plain space.
            KeyCode::Tab => {
                editor.insert(' ');
                true
            }
            _ => false,
        };
        if changed {
            redraw_boxed_input(&mut editor, origin_col, field_width);
        }
    };
    drop(guard);
    Some(outcome)
}

/// Redraws the editable field as one buffered write + flush — the same
/// atomic-write discipline as `PromptQuoteTicker::render_frame` — so a
/// concurrent quote frame can never interleave mid-sequence. `origin_col` is
/// 0-based (crossterm convention); the ANSI CHA sequence (`ESC[<n>G`) is
/// 1-based, hence the `+ 1`.
fn redraw_boxed_input(editor: &mut BoxedLineEditor, origin_col: u16, field_width: usize) {
    let (visible, cursor_offset) = editor.visible_window(field_width);
    let padding = " ".repeat(field_width.saturating_sub(visible.chars().count()));
    let text_column = origin_col as usize + 1;
    let cursor_column = origin_col as usize + cursor_offset + 1;
    print!("\x1b[?25l\x1b[{text_column}G{visible}{padding}\x1b[{cursor_column}G\x1b[?25h");
    std::io::stdout().flush().ok();
}

/// Single-line input state for the boxed prompt: buffer, cursor, and the
/// horizontal scroll window. Pure state and window math (no terminal I/O) so
/// it is unit-testable; each `char` is treated as one terminal column, the
/// same width model the rest of the TUI uses.
struct BoxedLineEditor {
    buffer: Vec<char>,
    cursor: usize,
    scroll: usize,
}

impl BoxedLineEditor {
    fn new() -> Self {
        BoxedLineEditor {
            buffer: Vec::new(),
            cursor: 0,
            scroll: 0,
        }
    }

    fn text(&self) -> String {
        self.buffer.iter().collect()
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    fn insert(&mut self, ch: char) {
        self.buffer.insert(self.cursor, ch);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.buffer.remove(self.cursor);
        true
    }

    fn delete_forward(&mut self) -> bool {
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.buffer.remove(self.cursor);
        true
    }

    fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    fn move_right(&mut self) -> bool {
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn move_home(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    fn move_end(&mut self) -> bool {
        if self.cursor == self.buffer.len() {
            return false;
        }
        self.cursor = self.buffer.len();
        true
    }

    fn kill_to_start(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.buffer.drain(..self.cursor);
        self.cursor = 0;
        true
    }

    fn kill_to_end(&mut self) -> bool {
        if self.cursor >= self.buffer.len() {
            return false;
        }
        self.buffer.truncate(self.cursor);
        true
    }

    fn delete_word_back(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let mut start = self.cursor;
        while start > 0 && self.buffer[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !self.buffer[start - 1].is_whitespace() {
            start -= 1;
        }
        self.buffer.drain(start..self.cursor);
        self.cursor = start;
        true
    }

    /// Clamps the scroll window so the cursor stays visible and returns the
    /// characters to draw plus the cursor's offset within the field. The last
    /// column doubles as the end-of-line cursor cell, so the window never
    /// pushes the cursor onto the box border.
    fn visible_window(&mut self, field_width: usize) -> (String, usize) {
        let width = field_width.max(2);
        let len = self.buffer.len();
        // Never leave dead space on the right while earlier text is hidden.
        self.scroll = self.scroll.min(len.saturating_sub(width - 1));
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor + 1 > self.scroll + width {
            self.scroll = self.cursor + 1 - width;
        }
        let end = (self.scroll + width).min(len);
        let visible: String = self.buffer[self.scroll..end].iter().collect();
        (visible, self.cursor - self.scroll)
    }
}

pub(crate) fn test_deepseek_connection(provider: &DeepSeekProvider) -> Result<(), CliError> {
    let model_id = provider
        .default_model()
        .cloned()
        .ok_or_else(|| CliError::new("DeepSeek provider has no default model"))?;
    let request = CompletionRequest::new(
        provider.id().clone(),
        model_id,
        vec![
            CompletionMessage::system("You are a connection test assistant."),
            CompletionMessage::user("Reply with only the single word: ready"),
        ],
    );
    provider
        .complete(request)
        .map(|_| ())
        .map_err(|error| match error {
            ProviderError::MissingApiKey | ProviderError::AuthenticationFailed(_) => {
                CliError::new("API key is invalid or not authorized. Please check your key at https://platform.deepseek.com/api_keys")
            }
            other => CliError::new(format!("connection test failed: {other}")),
        })
}

pub(crate) fn prompt_provider_and_api_key() -> Result<String, CliError> {
    use std::io::Write as _;

    println!();
    println!("{C_RED_BOLD}  ╔═══════════════════════════════════════════════════╗{C_RESET}");
    println!("{C_RED_BOLD}  ║{C_RESET}  {C_BOLD}⚡  CONFIGURE AI PROVIDER{C_RESET}                       {C_RED_BOLD}║{C_RESET}");
    println!("{C_RED_BOLD}  ╚═══════════════════════════════════════════════════╝{C_RESET}");
    println!();
    println!("  {C_DIM}Connect RaptorCode to an AI provider to start coding.{C_RESET}");
    println!("  {C_DIM}↑/↓ to navigate · Enter to select{C_RESET}");
    println!();

    let items = vec![
        MenuItem {
            label: "DeepSeek V4 Pro API  ─  agentic coding · platform.deepseek.com".to_owned(),
            sublabel: None,
            enabled: true,
        },
        MenuItem {
            label: "OpenRouter Key  [Coming Soon]".to_owned(),
            sublabel: None,
            enabled: false,
        },
        MenuItem {
            label: "Gemini API Key  [Coming Soon]".to_owned(),
            sublabel: None,
            enabled: false,
        },
    ];

    interactive_select(&items).ok_or_else(|| CliError::new("provider selection cancelled"))?;

    // Only DeepSeek is selectable for now
    println!();
    println!("  {C_BOLD}DeepSeek API{C_RESET} selected.");
    println!("  Get your API key at: {C_CYAN}{DEEPSEEK_API_KEY_URL}{C_RESET}");
    println!();
    print!("  {C_RED_BOLD}Paste your DeepSeek API key:{C_RESET} ");
    std::io::stdout().flush().ok();

    let mut key = String::new();
    std::io::stdin()
        .read_line(&mut key)
        .map_err(|error| CliError::new(format!("failed to read input: {error}")))?;

    let key = key.trim().to_owned();

    if key.is_empty() {
        return Err(CliError::new(
            "API key is required; run raptor again to enter your key",
        ));
    }

    if key.len() < 8 || key.contains(char::is_whitespace) {
        return Err(CliError::new(
            "API key appears invalid — please check and run raptor again",
        ));
    }

    Ok(key)
}

pub(crate) fn resolve_deepseek_api_key() -> Option<String> {
    if let Ok(key) = std::env::var(DEEPSEEK_API_KEY_ENV) {
        let key = key.trim().to_owned();
        if !key.is_empty() {
            return Some(key);
        }
    }
    read_stored_deepseek_api_key()
}

fn read_stored_deepseek_api_key() -> Option<String> {
    let path = user_credentials_path()?;
    let content = std::fs::read_to_string(path).ok()?;
    let key = content.trim().to_owned();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

pub(crate) fn write_stored_deepseek_api_key(key: &str) -> Result<(), CliError> {
    let dir = user_raptor_dir().ok_or_else(|| CliError::new("cannot determine home directory"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| CliError::new(format!("failed to create {}: {error}", dir.display())))?;
    let path = dir.join(".credentials");
    std::fs::write(&path, key)
        .map_err(|error| CliError::new(format!("failed to write credentials: {error}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                CliError::new(format!(
                    "failed to set credentials file permissions: {error}"
                ))
            },
        )?;
    }

    #[cfg(windows)]
    {
        restrict_windows_credentials_acl(&path)?;
    }

    Ok(())
}

/// Locks the credentials file down to the current user on Windows, the closest
/// equivalent to the Unix `0600` mode. `icacls /inheritance:r` strips inherited
/// ACEs (which can grant Administrators/SYSTEM read access) and `/grant:r` then
/// re-grants full control to the current user only. icacls is a built-in Windows
/// tool, so this needs no extra crate dependency — matching the existing pattern
/// of shelling out to `kill` for process control. Hard-fails like the Unix chmod
/// so an API key is never left readable by other principals.
#[cfg(windows)]
fn restrict_windows_credentials_acl(path: &Path) -> Result<(), CliError> {
    let username = std::env::var("USERNAME").map_err(|_| {
        CliError::new("cannot determine current Windows user to secure credentials file")
    })?;
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{username}:F"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|error| {
            CliError::new(format!(
                "failed to set credentials file permissions: {error}"
            ))
        })?;
    if !status.success() {
        return Err(CliError::new(
            "icacls failed to restrict credentials file permissions",
        ));
    }
    Ok(())
}

fn user_raptor_dir() -> Option<PathBuf> {
    // Unix/macOS key off $HOME; Windows has no $HOME by default, so fall back to
    // %USERPROFILE% (with $HOME honored first if a shell set it) — otherwise the
    // credentials path never resolves and the saved API key is unreachable.
    #[cfg(windows)]
    {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".raptor"))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".raptor"))
    }
}

fn user_credentials_path() -> Option<PathBuf> {
    user_raptor_dir().map(|dir| dir.join(".credentials"))
}

pub(crate) fn render_interactive_help() -> String {
    format!(
        "\n{C_BOLD}RaptorCode commands:{C_RESET}\n  /help or ?   Show this help\n  /swarm <mission>  Launch the live four-specialist agent swarm\n  /clear       Clear conversation history\n  /api         Change or view API key\n  /model       Select AI model\n  /thinking    Toggle DeepSeek thinking mode on/off\n  /effort      Toggle DeepSeek reasoning effort high/max\n  /stream      Toggle streaming responses\n  /json        Toggle DeepSeek JSON output mode\n  /balance     Show DeepSeek account balance\n  /exit        Exit\n  /quit        Exit\n\nJust type your coding request and press Enter.\n\n"
    )
}

fn prompt_file_access_consent(_current_dir: &Path) -> bool {
    use std::io::Write as _;

    println!();
    println!("{C_DIM}─────────────────────────────────────────────────────{C_RESET}");
    println!("  {C_YELLOW_BOLD}⚡  Agentic Mode{C_RESET}");
    println!();
    println!("  Allow RaptorCode to execute shell commands and modify files?");
    println!("  {C_DIM}Grants access to the full file system as your user.{C_RESET}");
    println!();
    print!("  Grant permission? [{C_GREEN_BOLD}y{C_RESET}/{C_DIM}N{C_RESET}]: ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    let answer = answer.trim().to_lowercase();
    answer == "y" || answer == "yes"
}

fn prompt_tool_policy_confirmation(message: &str) -> bool {
    use std::io::Write as _;

    println!();
    println!("  {C_YELLOW_BOLD}⚠  Policy check{C_RESET}");
    println!("  {message}");
    print!("  Allow? [y/{C_DIM}N{C_RESET}]: ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }

    let answer = answer.trim().to_lowercase();
    answer == "y" || answer == "yes"
}

const HACKING_QUOTES_RAW: &str = include_str!("../../../assets/200_hacking_quotes.md");

static HACKING_QUOTES: OnceLock<Vec<String>> = OnceLock::new();

pub(crate) fn hacking_quotes() -> &'static [String] {
    HACKING_QUOTES.get_or_init(|| {
        HACKING_QUOTES_RAW
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                let dot_pos = line.find(". ")?;
                let prefix = &line[..dot_pos];
                if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                    let text = line[dot_pos + 2..].trim();
                    if !text.is_empty() {
                        Some(text.to_owned())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    })
}

fn random_quote() -> &'static str {
    let quotes = hacking_quotes();
    if quotes.is_empty() {
        return "The best way to predict the future is to invent it";
    }
    &quotes[random_quote_index(quotes.len())]
}

fn random_quote_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(42);
    seed % len
}

/// Builds the rotating footer-quote pool for a live swarm run: the full quote set
/// rotated to start at a pseudo-random offset, so each run opens on a different
/// quote and the dashboard then cycles through the rest in order. Falls back to a
/// single default when no quotes are embedded.
pub(crate) fn swarm_quote_pool() -> Vec<String> {
    let quotes = hacking_quotes();
    if quotes.is_empty() {
        return vec!["The best way to predict the future is to invent it".to_owned()];
    }
    let start = random_quote_index(quotes.len());
    quotes[start..]
        .iter()
        .chain(quotes[..start].iter())
        .cloned()
        .collect()
}

/// Truncates a quote to fit the available terminal width, appending an ellipsis
/// when it had to cut. Counts characters (not bytes) so multi-byte glyphs stay
/// intact.
pub(crate) fn truncate_quote(quote: &str, max_chars: usize) -> String {
    if quote.chars().count() <= max_chars {
        return quote.to_owned();
    }
    let mut out: String = quote.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Decorative, rotating quote shown on a reserved line beneath the input box
/// while the developer is typing a prompt. It only writes to stdout (never reads
/// stdin) and restores the cursor after every frame so it does not disturb typing.
struct PromptQuoteTicker {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl PromptQuoteTicker {
    /// Starts rotating quotes on the line two rows below the input cursor. The
    /// caller must have reserved that line and positioned the cursor on the input
    /// line before calling this.
    fn start(width: u16) -> Self {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                Self::render_frame(width);
                // Sleep in short slices so a stop request is honored promptly.
                for _ in 0..25 {
                    if flag.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });

        PromptQuoteTicker {
            stop_flag,
            handle: Some(handle),
        }
    }

    fn render_frame(width: u16) {
        let max_chars = (width as usize).saturating_sub(6).max(8);
        let quote = truncate_quote(random_quote(), max_chars);
        // One atomic write: save cursor (DECSC), drop two rows to the reserved
        // quote line, clear it, draw the quote, then restore the cursor (DECRC).
        // Writing the whole sequence at once keeps the terminal from splitting it
        // across the echo of a keystroke, so typing is never displaced.
        print!("\x1b7\x1b[2B\r\x1b[2K  {C_DIM}{C_RED}{quote}{C_RESET}\x1b8");
        std::io::stdout().flush().ok();
    }

    fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }
}

struct Spinner {
    stop_flag: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

struct AgentLoopOutput {
    assistant_message: Option<AgentMessage>,
    content: String,
    streamed: bool,
    usage: Option<TokenUsage>,
}

struct AgentLoopContext<'a> {
    context_budget: &'a mut context_budget::ContextBudgetState,
    model_id: &'a ModelId,
    prompt_guard: &'a PromptGuard,
    provider: &'a DeepSeekProvider,
    request_options: &'a DeepSeekRequestOptions,
    streaming_enabled: bool,
    tool_policy: &'a mut ToolPolicy,
    tools: &'a [ToolDefinition],
}

impl Spinner {
    fn start(thinking: impl Into<String>) -> Self {
        let thinking = thinking.into();
        let quote = random_quote().to_owned();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop_flag);

        let handle = thread::spawn(move || {
            print!("\n\n\x1b[2A");
            std::io::stdout().flush().ok();
            let mut dot_idx = 0usize;
            while !flag.load(Ordering::Relaxed) {
                let dots = match dot_idx % 3 {
                    0 => ".",
                    1 => "..",
                    _ => "...",
                };
                print!(
                    "\r\x1b[K{C_DIM}{thinking}{C_RESET}\n\r\x1b[K{C_RED}{quote}{dots}{C_RESET}\x1b[1A\r"
                );
                std::io::stdout().flush().ok();
                dot_idx += 1;
                thread::sleep(Duration::from_millis(400));
            }
            print!("\r\x1b[K\n\r\x1b[K\x1b[1A\r");
            std::io::stdout().flush().ok();
        });

        Spinner {
            stop_flag,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            h.join().ok();
        }
    }
}

fn run_agent_loop(
    working_dir: &mut PathBuf,
    history: &mut Vec<AgentMessage>,
    context: &mut AgentLoopContext<'_>,
) -> Result<AgentLoopOutput, CliError> {
    for _ in 0..100 {
        let mut streamed_text = false;
        let step = if context.streaming_enabled {
            print!("  {C_DIM}Streaming agent response...{C_RESET}\n\n");
            std::io::stdout().flush().ok();
            let mut redactor = StreamRedactor::new(context.prompt_guard);
            let step = context.provider.complete_agent_step_streaming(
                context.model_id,
                history,
                context.tools,
                context.request_options,
                |delta| {
                    if delta.content.is_some() {
                        streamed_text = true;
                    }
                    if delta.reasoning_content.is_some() {
                        return;
                    }
                    if let Some(content) = delta.content {
                        print!("{}", redactor.push(&content));
                        std::io::stdout().flush().ok();
                    }
                },
            );
            print!("{}", redactor.finish());
            std::io::stdout().flush().ok();
            step
        } else {
            let spinner = Spinner::start("Thinking...");
            let step = context.provider.complete_agent_step(
                context.model_id,
                history,
                context.tools,
                context.request_options,
            );
            spinner.stop();
            step
        };

        match step {
            Ok(AgentStep {
                assistant_message,
                result: AgentStepResult::Text(content),
                usage,
            }) => {
                return Ok(AgentLoopOutput {
                    assistant_message: Some(assistant_message),
                    content,
                    streamed: streamed_text,
                    usage,
                });
            }
            Ok(AgentStep {
                assistant_message,
                result: AgentStepResult::ToolCalls(calls),
                usage,
            }) => {
                if let Some(usage) = usage.as_ref() {
                    let line = render_context_usage(usage);
                    if !line.is_empty() {
                        println!("  {C_DIM}{line}{C_RESET}");
                    }
                }
                history.push(assistant_message);

                for call in &calls {
                    let action = describe_tool_call(&call.name, &call.arguments);
                    println!("  {C_DIM}→ {action}{C_RESET}");
                    std::io::stdout().flush().ok();

                    let mut approved = true;
                    let result = match context.tool_policy.evaluate(
                        working_dir,
                        &call.name,
                        &call.arguments,
                    ) {
                        PolicyDecision::Deny { reason } => {
                            approved = false;
                            println!(
                                "  {C_RED}✗ blocked by policy{C_RESET}  {C_DIM}{reason}{C_RESET}"
                            );
                            format!(
                                "error: blocked by RaptorCode policy ({reason}). Choose a safer approach and continue."
                            )
                        }
                        PolicyDecision::Confirm(request) => {
                            if prompt_tool_policy_confirmation(&request.message) {
                                if request.kind == ConfirmationKind::OutsideWorkspaceWrite {
                                    context.tool_policy.allow_outside_writes_for_session();
                                    println!(
                                        "  {C_DIM}Outside-workspace writes allowed for this session.{C_RESET}"
                                    );
                                }
                                let spinner = Spinner::start(action.clone());
                                let result =
                                    execute_tool_call(working_dir, &call.name, &call.arguments);
                                spinner.stop();
                                result
                            } else {
                                approved = false;
                                println!("  {C_RED}✗ denied by developer{C_RESET}");
                                format!(
                                    "error: the developer declined this action ({}). Ask for guidance or choose a different approach.",
                                    request.message
                                )
                            }
                        }
                        PolicyDecision::Allow => {
                            let spinner = Spinner::start(action.clone());
                            let result =
                                execute_tool_call(working_dir, &call.name, &call.arguments);
                            spinner.stop();
                            result
                        }
                    };

                    if approved {
                        if result.starts_with("error:") {
                            println!("  {C_RED}✗ action needs adjustment{C_RESET}");
                        } else {
                            println!("  {C_GREEN}✓ done{C_RESET}");
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

                handle_agent_context_budget(history, usage.as_ref(), context.context_budget);
                println!();
            }
            Err(ProviderError::MissingApiKey) | Err(ProviderError::AuthenticationFailed(_)) => {
                return Err(CliError::new("authentication failed; check your API key"));
            }
            Err(error) => return Err(CliError::new(format!("agent error: {error}"))),
        }
    }

    Ok(AgentLoopOutput {
        assistant_message: None,
        content:
            "⏸  Step limit reached. All progress is saved — type **continue** to resume where I left off."
                .to_owned(),
        streamed: false,
        usage: None,
    })
}

fn handle_chat_context_budget(
    history: &mut Vec<CompletionMessage>,
    usage: Option<&TokenUsage>,
    state: &mut context_budget::ContextBudgetState,
) {
    for notice in context_budget::chat_notices(history, usage, state) {
        print_context_budget_notice(notice);
    }
}

fn handle_agent_context_budget(
    history: &mut Vec<AgentMessage>,
    usage: Option<&TokenUsage>,
    state: &mut context_budget::ContextBudgetState,
) {
    for notice in context_budget::agent_notices(history, usage, state) {
        print_context_budget_notice(notice);
    }
}

fn print_context_budget_notice(notice: context_budget::ContextBudgetNotice) {
    match notice {
        context_budget::ContextBudgetNotice::Warning {
            total_tokens,
            token_limit,
        } => {
            let percent = (total_tokens as f64 / token_limit as f64) * 100.0;
            println!(
                "  {C_YELLOW_BOLD}Context budget warning{C_RESET}  {C_DIM}{} / {} tokens ({percent:.1}%){C_RESET}",
                display_number(total_tokens),
                display_number(token_limit)
            );
        }
        context_budget::ContextBudgetNotice::Compacted { omitted, retained } => {
            println!(
                "  {C_GREEN}✓ context compacted{C_RESET}  {C_DIM}omitted {omitted} older messages, retained {retained} recent messages{C_RESET}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_with(text: &str) -> BoxedLineEditor {
        let mut editor = BoxedLineEditor::new();
        for ch in text.chars() {
            editor.insert(ch);
        }
        editor
    }

    #[test]
    fn short_input_fits_without_scrolling() {
        let mut editor = editor_with("hello");
        let (visible, cursor) = editor.visible_window(10);

        assert_eq!(visible, "hello");
        assert_eq!(cursor, 5);
        assert_eq!(editor.scroll, 0);
    }

    #[test]
    fn typing_past_the_field_scrolls_instead_of_overflowing() {
        let mut editor = editor_with("abcdefghijklmnopqrstuvwxyz1234");
        let (visible, cursor) = editor.visible_window(10);

        // The window shows the tail with the last column reserved for the
        // end-of-line cursor, so nothing can ever cross the box border.
        assert_eq!(visible, "vwxyz1234");
        assert_eq!(cursor, 9);
        assert!(visible.chars().count() <= 10);
    }

    #[test]
    fn window_and_cursor_never_exceed_the_field() {
        let mut editor = BoxedLineEditor::new();
        for step in 0..200 {
            editor.insert(char::from(b'a' + (step % 26) as u8));
            let (visible, cursor) = editor.visible_window(12);
            assert!(visible.chars().count() <= 12, "window overflow at {step}");
            assert!(cursor < 12, "cursor past field at {step}");
        }
    }

    #[test]
    fn home_scrolls_back_to_the_start() {
        let mut editor = editor_with("abcdefghijklmnopqrstuvwxyz");
        editor.visible_window(10);
        assert!(editor.move_home());
        let (visible, cursor) = editor.visible_window(10);

        assert_eq!(visible, "abcdefghij");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn arrows_walk_the_window_across_long_input() {
        let mut editor = editor_with("abcdefghijklmnopqrstuvwxyz");
        editor.move_home();
        editor.visible_window(10);
        for _ in 0..15 {
            editor.move_right();
        }
        let (visible, cursor) = editor.visible_window(10);

        // Cursor sits on 'p' (index 15); the window scrolled just enough to
        // keep it on the last visible column.
        assert_eq!(cursor, 9);
        assert_eq!(visible, "ghijklmnop");
        assert_eq!(editor.scroll, 6);
    }

    #[test]
    fn backspace_and_delete_edit_at_the_cursor() {
        let mut editor = editor_with("hello world");
        for _ in 0..5 {
            editor.move_left();
        }
        assert!(editor.backspace()); // removes the space before "world"
        assert_eq!(editor.text(), "helloworld");
        assert!(editor.delete_forward()); // removes the 'w' under the cursor
        assert_eq!(editor.text(), "helloorld");
    }

    #[test]
    fn kill_and_word_deletion_shortcuts() {
        let mut editor = editor_with("cargo build --release  ");
        assert!(editor.delete_word_back());
        assert_eq!(editor.text(), "cargo build ");

        assert!(editor.kill_to_start());
        assert_eq!(editor.text(), "");
        assert!(editor.is_empty());
        let (visible, cursor) = editor.visible_window(10);
        assert_eq!(visible, "");
        assert_eq!(cursor, 0);

        let mut editor = editor_with("keep this");
        editor.move_home();
        for _ in 0..4 {
            editor.move_right();
        }
        assert!(editor.kill_to_end());
        assert_eq!(editor.text(), "keep");
    }

    #[test]
    fn deleting_pulls_the_window_back_from_dead_space() {
        let mut editor = editor_with("abcdefghijklmnopqrstuvwxyz");
        editor.visible_window(10);
        assert!(editor.scroll > 0);
        for _ in 0..20 {
            editor.backspace();
        }
        let (visible, cursor) = editor.visible_window(10);

        // Only "abcdef" remains: the window must snap back to show it all
        // rather than leaving a stale scroll offset and a blank field.
        assert_eq!(visible, "abcdef");
        assert_eq!(cursor, 6);
        assert_eq!(editor.scroll, 0);
    }

    #[test]
    fn mid_text_insertion_keeps_order() {
        let mut editor = editor_with("raptor");
        for _ in 0..3 {
            editor.move_left();
        }
        editor.insert('X');
        assert_eq!(editor.text(), "rapXtor");
        let (_, cursor) = editor.visible_window(20);
        assert_eq!(cursor, 4);
    }
}
