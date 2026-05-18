use std::env;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, size, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::{Frame, Terminal};

pub const CRATE_PURPOSE: &str = "Terminal user interface boundaries for RaptorCode.";
pub const RAPTOR_LABS_URL: &str = "https://raptorlabs.dev";

pub const BRAND_BACKGROUND_HEX: &str = "0A0A0A";
pub const BRAND_PRIMARY_TEXT_HEX: &str = "F4F4F4";
pub const BRAND_SECONDARY_TEXT_HEX: &str = "AAAAA9";
pub const BRAND_MUTED_TEXT_HEX: &str = "636262";
pub const BRAND_RED_HEX: &str = "FF1919";
pub const BRAND_DARK_RED_HEX: &str = "A51D20";
pub const BRAND_SUBTLE_DEEP_RED_HEX: &str = "261313";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    None,
    Ansi16,
    Truecolor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrandToken {
    BrandRed,
    DarkRed,
    Muted,
    Primary,
    Secondary,
    SubtleDeepRed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBannerOptions {
    pub api_key_url: String,
    pub ci: bool,
    pub color_mode: ColorMode,
    pub compact: bool,
    pub cwd: PathBuf,
    pub deepseek_api_key_present: bool,
    pub interactive: bool,
    pub mode: Option<String>,
    pub model_name: Option<String>,
    pub terminal_width: u16,
    pub user_name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardState {
    pub agent_roles: Vec<AgentDashboardSummary>,
    pub changed_file_count: Option<usize>,
    pub cwd: PathBuf,
    pub deepseek_api_key_present: bool,
    pub model_name: String,
    pub provider_name: String,
    pub swarms: Vec<SwarmDashboardSummary>,
    pub tasks: Vec<TaskDashboardSummary>,
    pub test_status: Option<String>,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDashboardSummary {
    pub planning_status: Option<String>,
    pub proposal_status: Option<String>,
    pub role: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmDashboardSummary {
    pub id: String,
    pub planning_status: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDashboardSummary {
    pub id: String,
    pub status: String,
}

impl StartupBannerOptions {
    pub fn plain(version: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            api_key_url: "https://platform.deepseek.com/api_keys".to_owned(),
            ci: true,
            color_mode: ColorMode::None,
            compact: true,
            cwd: cwd.into(),
            deepseek_api_key_present: false,
            interactive: false,
            mode: None,
            model_name: None,
            terminal_width: 80,
            user_name: "developer".to_owned(),
            version: version.into(),
        }
    }
}

impl DashboardState {
    pub fn empty(version: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            agent_roles: vec![
                AgentDashboardSummary::pending("frontend-agent"),
                AgentDashboardSummary::pending("backend-agent"),
                AgentDashboardSummary::pending("test-agent"),
                AgentDashboardSummary::pending("security-agent"),
            ],
            changed_file_count: None,
            cwd: cwd.into(),
            deepseek_api_key_present: false,
            model_name: "deepseek-chat".to_owned(),
            provider_name: "DeepSeek".to_owned(),
            swarms: Vec::new(),
            tasks: Vec::new(),
            test_status: None,
            version: version.into(),
        }
    }
}

impl AgentDashboardSummary {
    pub fn pending(role: impl Into<String>) -> Self {
        Self {
            planning_status: None,
            proposal_status: None,
            role: role.into(),
            status: "not_started".to_owned(),
        }
    }
}

pub fn color_mode_from_environment(interactive: bool, ci: bool) -> ColorMode {
    if !interactive || ci || env::var_os("NO_COLOR").is_some() {
        return ColorMode::None;
    }

    let colorterm = env::var("COLORTERM").unwrap_or_default().to_lowercase();
    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        ColorMode::Truecolor
    } else {
        ColorMode::Ansi16
    }
}

pub fn terminal_width_from_environment() -> u16 {
    size().map(|(width, _)| width).unwrap_or(80)
}

pub fn render_startup_banner(options: &StartupBannerOptions) -> String {
    if !options.interactive || options.ci {
        return format!(
            "Raptor Code {} - Raptor Labs\n",
            display_version(&options.version)
        );
    }

    if options.compact || options.terminal_width < 70 {
        return render_compact_startup_banner(options);
    }

    if options.terminal_width >= 100 {
        render_wide_startup_banner(options)
    } else {
        render_stacked_startup_banner(options)
    }
}

pub fn render_dashboard_snapshot(state: &DashboardState) -> String {
    let mut output = format!(
        "Raptor Code {}\nRaptor Labs\nProvider/model: {} / {}\nWorking directory: {}\nDeepSeek API key: {}\nTasks: {}\nSwarms: {}\nChanged files: {}\nTest status: {}\n\nAgents:\n",
        display_version(&state.version),
        state.provider_name,
        state.model_name,
        state.cwd.display(),
        api_key_status(state.deepseek_api_key_present),
        state.tasks.len(),
        state.swarms.len(),
        optional_count(state.changed_file_count),
        state.test_status.as_deref().unwrap_or("not recorded"),
    );

    for agent in &state.agent_roles {
        output.push_str(&format!(
            "  {} - status: {}, planning: {}, implementation: {}\n",
            agent.role,
            agent.status,
            agent.planning_status.as_deref().unwrap_or("-"),
            agent.proposal_status.as_deref().unwrap_or("-"),
        ));
    }

    output.push_str("\nControls: q to quit, /help for commands\n");
    output.push_str("Dashboard mode: read-only; no model calls; no file writes\n");
    output
}

pub fn run_dashboard(state: DashboardState) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_dashboard_loop(&mut terminal, &state);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &DashboardState,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| draw_dashboard(frame, state))?;

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn draw_dashboard(frame: &mut Frame<'_>, state: &DashboardState) {
    let area = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Min(7),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "[ RC ] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("Raptor Code {}", display_version(&state.version)),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            "Raptor Labs",
            Style::default().fg(Color::LightRed),
        )),
        Line::from(Span::styled(
            "Agentic software engineering at predator speed.",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            RAPTOR_LABS_URL,
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .title(" RaptorCode ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    );
    frame.render_widget(header, chunks[0]);

    let meta = Paragraph::new(vec![
        Line::from(format!("Working directory: {}", state.cwd.display())),
        Line::from(format!(
            "Provider/model: {} / {}",
            state.provider_name, state.model_name
        )),
        Line::from(format!(
            "DeepSeek API key: {}",
            api_key_status(state.deepseek_api_key_present)
        )),
        Line::from(format!("Tasks: {}", state.tasks.len())),
        Line::from(format!("Swarms: {}", state.swarms.len())),
        Line::from(format!(
            "Changed files: {}",
            optional_count(state.changed_file_count)
        )),
    ])
    .block(
        Block::default()
            .title(" Workspace ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .style(Style::default().fg(Color::Gray));
    frame.render_widget(meta, chunks[1]);

    let agents = state
        .agent_roles
        .iter()
        .map(|agent| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    agent.role.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  status: "),
                Span::styled(agent.status.clone(), Style::default().fg(Color::LightRed)),
                Span::raw("  planning: "),
                Span::raw(agent.planning_status.as_deref().unwrap_or("-")),
                Span::raw("  implementation: "),
                Span::raw(agent.proposal_status.as_deref().unwrap_or("-")),
            ]))
        })
        .collect::<Vec<_>>();
    let agents = List::new(agents).block(
        Block::default()
            .title(" v0.1 Agents ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(agents, chunks[2]);

    let footer = Paragraph::new("q to quit  |  /help for commands  |  read-only dashboard")
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, chunks[3]);
}

fn render_compact_startup_banner(options: &StartupBannerOptions) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "{}\n",
        paint(options.color_mode, BrandToken::BrandRed, "RAPTOR LABS")
    ));
    output.push_str(&format!(
        "{}\n",
        paint(
            options.color_mode,
            BrandToken::Primary,
            &format!("Raptor Code {}", display_version(&options.version))
        )
    ));
    output.push_str(&format!(
        "Welcome, {}. Enjoy your coding session. Ship clean.\n",
        options.user_name
    ));
    output.push_str(&startup_api_key_line(options));
    output
}

fn render_wide_startup_banner(options: &StartupBannerOptions) -> String {
    let width = bounded_width(options.terminal_width, 96);
    let separator = paint(
        options.color_mode,
        BrandToken::SubtleDeepRed,
        &"-".repeat(width as usize),
    );
    let headline = format!("Raptor Code {}", display_version(&options.version));
    let mut output = String::new();

    output.push_str(&separator);
    output.push('\n');
    output.push_str(&format!(
        "  {}  {}\n",
        paint(
            options.color_mode,
            BrandToken::BrandRed,
            "[ RC ] RAPTOR CODE"
        ),
        paint(options.color_mode, BrandToken::Primary, "RAPTOR LABS")
    ));
    output.push_str(&format!(
        "  {}  {}\n\n",
        paint(
            options.color_mode,
            BrandToken::DarkRed,
            "terminal engineering system"
        ),
        paint(options.color_mode, BrandToken::Secondary, RAPTOR_LABS_URL)
    ));
    output.push_str(&format!(
        "  {}\n",
        paint(options.color_mode, BrandToken::Primary, &headline)
    ));
    output.push_str(&format!(
        "  {}\n\n",
        paint(
            options.color_mode,
            BrandToken::Secondary,
            "Agentic software engineering at predator speed."
        )
    ));
    push_greeting(options, &mut output);
    push_metadata(options, &mut output);
    output.push_str(&format!(
        "\n  {}\n",
        paint(
            options.color_mode,
            BrandToken::Muted,
            "Type /help for commands"
        )
    ));
    output.push_str(&separator);
    output.push('\n');

    output
}

fn render_stacked_startup_banner(options: &StartupBannerOptions) -> String {
    let width = bounded_width(options.terminal_width, 78);
    let separator = paint(
        options.color_mode,
        BrandToken::SubtleDeepRed,
        &"-".repeat(width as usize),
    );
    let mut output = String::new();

    output.push_str(&separator);
    output.push('\n');
    output.push_str(&format!(
        "  {}\n",
        paint(
            options.color_mode,
            BrandToken::BrandRed,
            "[ RC ] RAPTOR CODE"
        )
    ));
    output.push_str(&format!(
        "  {}\n",
        paint(options.color_mode, BrandToken::Primary, "RAPTOR LABS")
    ));
    output.push_str(&format!(
        "  {}\n\n",
        paint(options.color_mode, BrandToken::Secondary, RAPTOR_LABS_URL)
    ));
    output.push_str(&format!(
        "  {}\n",
        paint(
            options.color_mode,
            BrandToken::Primary,
            &format!("Raptor Code {}", display_version(&options.version))
        )
    ));
    output.push_str(&format!(
        "  {}\n\n",
        paint(
            options.color_mode,
            BrandToken::Secondary,
            "Agentic software engineering at predator speed."
        )
    ));
    push_greeting(options, &mut output);
    push_metadata(options, &mut output);
    output.push_str(&format!(
        "\n  {}\n",
        paint(
            options.color_mode,
            BrandToken::Muted,
            "Type /help for commands"
        )
    ));
    output.push_str(&separator);
    output.push('\n');

    output
}

fn push_greeting(options: &StartupBannerOptions, output: &mut String) {
    output.push_str(&format!(
        "  {}\n",
        paint(
            options.color_mode,
            BrandToken::Primary,
            &format!("Welcome, {}.", options.user_name)
        )
    ));
    output.push_str(&format!(
        "  {}\n\n",
        paint(
            options.color_mode,
            BrandToken::Secondary,
            "The Raptor Labs team wishes you an enjoyable coding session and a clean, successful delivery."
        )
    ));
}

fn push_metadata(options: &StartupBannerOptions, output: &mut String) {
    output.push_str(&format!(
        "  {} {}\n",
        paint(
            options.color_mode,
            BrandToken::Secondary,
            "Working directory:"
        ),
        options.cwd.display()
    ));
    output.push_str(&format!(
        "  {} {}\n",
        paint(options.color_mode, BrandToken::Secondary, "Model:"),
        options
            .model_name
            .as_deref()
            .unwrap_or("DeepSeek / deepseek-chat")
    ));
    output.push_str(&format!(
        "  {} {}\n",
        paint(options.color_mode, BrandToken::Secondary, "Mode:"),
        options.mode.as_deref().unwrap_or("Supervised v0.1")
    ));
    output.push_str(&startup_api_key_line(options));
}

fn startup_api_key_line(options: &StartupBannerOptions) -> String {
    if options.deepseek_api_key_present {
        format!(
            "  {} {}\n",
            paint(
                options.color_mode,
                BrandToken::Secondary,
                "DeepSeek API key:"
            ),
            paint(options.color_mode, BrandToken::BrandRed, "detected")
        )
    } else {
        format!(
            "  {} missing\n  Get a DeepSeek API key at {}\n",
            paint(
                options.color_mode,
                BrandToken::Secondary,
                "DeepSeek API key:"
            ),
            options.api_key_url
        )
    }
}

fn display_version(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        return trimmed.to_owned();
    }

    let mut parts = trimmed.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("v{major}.{minor}"),
        _ => format!("v{trimmed}"),
    }
}

fn api_key_status(present: bool) -> &'static str {
    if present {
        "detected"
    } else {
        "missing"
    }
}

fn optional_count(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "not calculated".to_owned())
}

fn bounded_width(terminal_width: u16, max_width: u16) -> u16 {
    terminal_width.clamp(40, max_width)
}

fn paint(mode: ColorMode, token: BrandToken, value: &str) -> String {
    match mode {
        ColorMode::None => value.to_owned(),
        ColorMode::Ansi16 => format!("\x1b[{}m{value}\x1b[0m", ansi16_code(token)),
        ColorMode::Truecolor => {
            let (red, green, blue) = truecolor_rgb(token);
            format!("\x1b[38;2;{red};{green};{blue}m{value}\x1b[0m")
        }
    }
}

fn ansi16_code(token: BrandToken) -> &'static str {
    match token {
        BrandToken::BrandRed => "91",
        BrandToken::DarkRed => "31",
        BrandToken::Muted => "90",
        BrandToken::Primary => "97",
        BrandToken::Secondary => "37",
        BrandToken::SubtleDeepRed => "31",
    }
}

fn truecolor_rgb(token: BrandToken) -> (u8, u8, u8) {
    let hex = match token {
        BrandToken::BrandRed => BRAND_RED_HEX,
        BrandToken::DarkRed => BRAND_DARK_RED_HEX,
        BrandToken::Muted => BRAND_MUTED_TEXT_HEX,
        BrandToken::Primary => BRAND_PRIMARY_TEXT_HEX,
        BrandToken::Secondary => BRAND_SECONDARY_TEXT_HEX,
        BrandToken::SubtleDeepRed => BRAND_SUBTLE_DEEP_RED_HEX,
    };

    (
        u8::from_str_radix(&hex[0..2], 16).unwrap_or(244),
        u8::from_str_radix(&hex[2..4], 16).unwrap_or(244),
        u8::from_str_radix(&hex[4..6], 16).unwrap_or(244),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn startup_options() -> StartupBannerOptions {
        StartupBannerOptions {
            api_key_url: "https://platform.deepseek.com/api_keys".to_owned(),
            ci: false,
            color_mode: ColorMode::None,
            compact: false,
            cwd: PathBuf::from("/workspace/project"),
            deepseek_api_key_present: false,
            interactive: true,
            mode: Some("Supervised v0.1".to_owned()),
            model_name: Some("DeepSeek / deepseek-chat".to_owned()),
            terminal_width: 100,
            user_name: "developer".to_owned(),
            version: "0.1.0".to_owned(),
        }
    }

    #[test]
    fn banner_renders_required_brand_content() {
        let output = render_startup_banner(&startup_options());

        assert!(output.contains("RAPTOR LABS"));
        assert!(output.contains("Raptor Code v0.1"));
        assert!(output.contains(RAPTOR_LABS_URL));
        assert!(output.contains("Welcome, developer."));
        assert!(output.contains("https://platform.deepseek.com/api_keys"));
    }

    #[test]
    fn no_color_banner_has_no_ansi_codes() {
        let output = render_startup_banner(&startup_options());

        assert!(!output.contains("\x1b["));
    }

    #[test]
    fn truecolor_banner_uses_ansi_codes() {
        let mut options = startup_options();
        options.color_mode = ColorMode::Truecolor;

        let output = render_startup_banner(&options);

        assert!(output.contains("\x1b[38;2;255;25;25m"));
    }

    #[test]
    fn non_interactive_banner_is_plain() {
        let mut options = startup_options();
        options.interactive = false;

        let output = render_startup_banner(&options);

        assert_eq!(output, "Raptor Code v0.1 - Raptor Labs\n");
    }

    #[test]
    fn compact_banner_avoids_oversized_logo() {
        let mut options = startup_options();
        options.terminal_width = 60;

        let output = render_startup_banner(&options);

        assert!(output.contains("RAPTOR LABS"));
        assert!(!output.contains("████"));
        assert!(output.contains("Ship clean."));
    }

    #[test]
    fn dashboard_snapshot_renders_core_state() {
        let mut state = DashboardState::empty("0.1.0", "/workspace/project");
        state.deepseek_api_key_present = true;
        state.agent_roles[0].status = "planning_completed".to_owned();

        let output = render_dashboard_snapshot(&state);

        assert!(output.contains("Raptor Code v0.1"));
        assert!(output.contains("Raptor Labs"));
        assert!(output.contains("DeepSeek / deepseek-chat"));
        assert!(output.contains("frontend-agent"));
        assert!(output.contains("q to quit"));
    }
}
