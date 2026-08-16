//! Terminal UI built on ratatui: the splash screen, status views, and the
//! live swarm dashboard.

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

mod swarm_dashboard;

pub use swarm_dashboard::{
    apply_swarm_event, render_swarm_snapshot, run_swarm_dashboard, SwarmAgentPhase,
    SwarmAgentUiState, SwarmDashboardState, SwarmUiEvent,
};

pub const CRATE_PURPOSE: &str = "Terminal user interface boundaries for RaptorCode.";
pub const RAPTOR_LABS_URL: &str = "https://raptorlabs.dev";

const KEY_ART_RAPTOR: [&str; 6] = [
    "██████╗  █████╗ ██████╗ ████████╗ ██████╗ ██████╗ ",
    "██╔══██╗██╔══██╗██╔══██╗╚══██╔══╝██╔═══██╗██╔══██╗",
    "██████╔╝███████║██████╔╝   ██║   ██║   ██║██████╔╝ ",
    "██╔══██╗██╔══██║██╔═══╝    ██║   ██║   ██║██╔══██╗ ",
    "██║  ██║██║  ██║██║        ██║   ╚██████╔╝██║  ██║ ",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝        ╚═╝    ╚═════╝ ╚═╝  ╚═╝",
];

const KEY_ART_CODE: [&str; 6] = [
    " ██████╗  ██████╗ ██████╗ ███████╗",
    "██╔════╝ ██╔═══██╗██╔══██╗██╔════╝",
    "██║      ██║   ██║██║  ██║█████╗  ",
    "██║      ██║   ██║██║  ██║██╔══╝  ",
    "╚██████╗ ╚██████╔╝██████╔╝███████╗",
    " ╚═════╝  ╚═════╝ ╚═════╝ ╚══════╝",
];

pub mod theme {
    pub const RAPTOR_RED: &str = "E63946";
    pub const RAPTOR_RED_MUTED: &str = "9E2A2F";
    pub const TEXT: &str = "D7D7D7";
    pub const DIM: &str = "7A7A7A";
    pub const ACCENT: &str = "FFFFFF";

    /// Live swarm dashboard truecolor palette. Deliberately hotter than the
    /// startup-banner `RAPTOR_RED` so live agent activity reads as high-energy,
    /// but owned here so every brand color has a single home.
    pub const LIVE_RED_RGB: (u8, u8, u8) = (0xFF, 0x19, 0x19);
    pub const LIVE_DIM_RGB: (u8, u8, u8) = (0x63, 0x62, 0x62);
    pub const LIVE_TRACK_RGB: (u8, u8, u8) = (0x3A, 0x20, 0x20);
}

pub const BRAND_BACKGROUND_HEX: &str = "0A0A0A";
pub const BRAND_PRIMARY_TEXT_HEX: &str = theme::ACCENT;
pub const BRAND_SECONDARY_TEXT_HEX: &str = theme::TEXT;
pub const BRAND_MUTED_TEXT_HEX: &str = theme::DIM;
pub const BRAND_RED_HEX: &str = theme::RAPTOR_RED;
pub const BRAND_DARK_RED_HEX: &str = theme::RAPTOR_RED_MUTED;
pub const BRAND_SUBTLE_DEEP_RED_HEX: &str = theme::RAPTOR_RED_MUTED;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    None,
    Ansi16,
    Ansi256,
    Truecolor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrandToken {
    Accent,
    Dim,
    RaptorRed,
    RaptorRedMuted,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBannerOptions {
    pub api_key_url: String,
    pub ci: bool,
    pub color_mode: ColorMode,
    pub compact: bool,
    pub cwd: PathBuf,
    pub deepseek_api_key_present: bool,
    pub emoji: bool,
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
            emoji: true,
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
            model_name: "DeepSeek V4 Pro".to_owned(),
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

    if let Ok(force_color) = env::var("FORCE_COLOR") {
        let force_color = force_color.trim().to_lowercase();
        if matches!(force_color.as_str(), "0" | "false" | "off" | "no") {
            return ColorMode::None;
        }
        if matches!(force_color.as_str(), "3" | "truecolor" | "24bit") {
            return ColorMode::Truecolor;
        }
        if matches!(force_color.as_str(), "2" | "256") {
            return ColorMode::Ansi256;
        }
        return ColorMode::Ansi16;
    }

    let colorterm = env::var("COLORTERM").unwrap_or_default().to_lowercase();
    if colorterm.contains("truecolor") || colorterm.contains("24bit") {
        return ColorMode::Truecolor;
    }

    let term = env::var("TERM").unwrap_or_default().to_lowercase();
    if term.contains("256color") {
        return ColorMode::Ansi256;
    }

    ColorMode::Ansi16
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

    if options.compact || options.terminal_width < 80 {
        return render_compact_startup_banner(options);
    }

    render_premium_startup_banner(options)
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

    push_compact_header(options, &mut output);
    output.push('\n');
    push_separator(options, &mut output);
    output.push('\n');
    push_title_block(options, &mut output);
    output.push('\n');
    push_greeting(options, &mut output);
    output.push('\n');
    push_metadata(options, &mut output);
    trim_trailing_newline(&mut output);

    output
}

fn render_premium_startup_banner(options: &StartupBannerOptions) -> String {
    let mut output = String::new();

    push_header(options, &mut output);
    output.push('\n');
    push_separator(options, &mut output);
    output.push('\n');
    push_title_block(options, &mut output);
    output.push('\n');
    push_greeting(options, &mut output);
    output.push('\n');
    push_metadata(options, &mut output);
    trim_trailing_newline(&mut output);

    output
}

fn push_header(options: &StartupBannerOptions, output: &mut String) {
    for line in &KEY_ART_RAPTOR {
        output.push_str("  ");
        output.push_str(&style(options.color_mode, BrandToken::Accent, true, line));
        output.push('\n');
    }
    for line in &KEY_ART_CODE {
        output.push_str("  ");
        output.push_str(&style(
            options.color_mode,
            BrandToken::RaptorRed,
            true,
            line,
        ));
        output.push('\n');
    }
    output.push_str("  ");
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "by "));
    output.push_str(&style(
        options.color_mode,
        BrandToken::Text,
        true,
        "Raptor Labs",
    ));
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "  ·  "));
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        &hyperlink(
            options.color_mode,
            "raptorlabs.dev",
            "https://raptorlabs.dev",
        ),
    ));
    output.push('\n');
}

fn trim_trailing_newline(output: &mut String) {
    if output.ends_with('\n') {
        output.pop();
    }
}

fn push_compact_header(options: &StartupBannerOptions, output: &mut String) {
    output.push_str("  ");
    output.push_str(&style(
        options.color_mode,
        BrandToken::Accent,
        true,
        "RAPTOR",
    ));
    output.push(' ');
    output.push_str(&style(
        options.color_mode,
        BrandToken::RaptorRed,
        true,
        "CODE",
    ));
    output.push_str(&paint(options.color_mode, BrandToken::Dim, " — "));
    output.push_str(&style(
        options.color_mode,
        BrandToken::RaptorRed,
        true,
        "Raptor Labs",
    ));
    output.push('\n');
}

fn push_title_block(options: &StartupBannerOptions, output: &mut String) {
    output.push_str("  ");
    output.push_str(&style(
        options.color_mode,
        BrandToken::Accent,
        true,
        "Raptor",
    ));
    output.push(' ');
    output.push_str(&style(
        options.color_mode,
        BrandToken::RaptorRed,
        true,
        "Code",
    ));
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "  "));
    output.push_str(&style(
        options.color_mode,
        BrandToken::Dim,
        false,
        &display_version(&options.version),
    ));
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        "  ·  agent runtime ready",
    ));
    output.push('\n');
    output.push_str("  ");
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Text,
        "Agentic software engineering at predator speed.",
    ));
    output.push('\n');
}

fn push_greeting(options: &StartupBannerOptions, output: &mut String) {
    let icon = if options.emoji { "[ 🦷 ]" } else { "[/\\]" };
    let continuation = "           ";

    output.push_str("  ");
    output.push_str(&paint(options.color_mode, BrandToken::RaptorRed, icon));
    output.push_str(if options.emoji { "   " } else { "     " });
    output.push_str(&style(
        options.color_mode,
        BrandToken::Accent,
        true,
        &format!("Welcome, {}.", options.user_name),
    ));
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        "  Agents standing by.",
    ));
    output.push('\n');
    output.push_str(continuation);
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        "Context scanned · toolchain ready · awaiting your command.",
    ));
    output.push('\n');
}

fn push_metadata(options: &StartupBannerOptions, output: &mut String) {
    let rows = metadata_rows(options);
    let inner_width = metadata_inner_width(options, &rows);

    push_metadata_border(options, output, inner_width, true);
    for row in rows {
        push_metadata_row(options, output, inner_width, row);
    }
    push_metadata_border(options, output, inner_width, false);
}

#[derive(Debug, Clone)]
struct MetadataRow {
    icon: &'static str,
    label: &'static str,
    value: String,
}

fn metadata_rows(options: &StartupBannerOptions) -> [MetadataRow; 4] {
    let (directory_icon, model_icon, mode_icon) = if options.emoji {
        ("📁", "🔗", "⏱")
    } else {
        ("[]", "<>", "::")
    };

    [
        MetadataRow {
            icon: directory_icon,
            label: "Working directory",
            value: options.cwd.display().to_string(),
        },
        MetadataRow {
            icon: model_icon,
            label: "Model",
            value: options
                .model_name
                .as_deref()
                .unwrap_or("DeepSeek")
                .to_owned(),
        },
        MetadataRow {
            icon: mode_icon,
            label: "Mode",
            value: options.mode.as_deref().unwrap_or("high effort").to_owned(),
        },
        MetadataRow {
            icon: ">_",
            label: "Type",
            value: "/help for commands".to_owned(),
        },
    ]
}

fn metadata_inner_width(options: &StartupBannerOptions, rows: &[MetadataRow]) -> usize {
    let max_row_width = rows.iter().map(metadata_row_width).max().unwrap_or(60);
    let max_terminal_width = options.terminal_width.saturating_sub(4) as usize;

    max_row_width
        .max(60)
        .min(max_terminal_width.max(48))
        .min(76)
}

fn metadata_row_width(row: &MetadataRow) -> usize {
    let icon = icon_field(row.icon);
    visible_width(" ")
        + visible_width(&icon)
        + visible_width(" ")
        + 18
        + visible_width(": ")
        + visible_width(&row.value)
        + visible_width(" ")
}

fn push_separator(options: &StartupBannerOptions, output: &mut String) {
    let width = separator_width(options);
    let left_width = width.saturating_sub(1) / 2;
    let right_width = width.saturating_sub(left_width + 1);

    output.push_str("  ");
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        &"─".repeat(left_width),
    ));
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "─"));
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        &"─".repeat(right_width),
    ));
    output.push('\n');
    output.push_str("  ");
    output.push_str(&" ".repeat(left_width));
    output.push_str(&paint(options.color_mode, BrandToken::RaptorRedMuted, "┬"));
    output.push('\n');
}

fn separator_width(options: &StartupBannerOptions) -> usize {
    (options.terminal_width.saturating_sub(4) as usize).clamp(44, 70)
}

fn push_metadata_row(
    options: &StartupBannerOptions,
    output: &mut String,
    inner_width: usize,
    row: MetadataRow,
) {
    let icon = icon_field(row.icon);
    let label = fit_field(row.label, 18);
    let fixed_width = visible_width(" ")
        + visible_width(&icon)
        + visible_width(" ")
        + visible_width(&label)
        + visible_width(": ")
        + visible_width(" ");
    let value_width = inner_width.saturating_sub(fixed_width);
    let value = fit_visible(&row.value, value_width);
    let value_padding = " ".repeat(value_width.saturating_sub(visible_width(&value)));

    output.push_str("  ");
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "│"));
    output.push(' ');
    output.push_str(&paint(options.color_mode, BrandToken::RaptorRed, &icon));
    output.push(' ');
    output.push_str(&paint(options.color_mode, BrandToken::RaptorRed, &label));
    output.push_str(&paint(options.color_mode, BrandToken::RaptorRed, ": "));
    output.push_str(&paint(options.color_mode, BrandToken::Text, &value));
    output.push_str(&value_padding);
    output.push(' ');
    output.push_str(&paint(options.color_mode, BrandToken::Dim, "│"));
    output.push('\n');
}

fn push_metadata_border(
    options: &StartupBannerOptions,
    output: &mut String,
    inner_width: usize,
    top: bool,
) {
    let (left, right) = if top { ("╭", "╮") } else { ("╰", "╯") };
    output.push_str("  ");
    output.push_str(&paint(options.color_mode, BrandToken::Dim, left));
    output.push_str(&paint(
        options.color_mode,
        BrandToken::Dim,
        &"─".repeat(inner_width),
    ));
    output.push_str(&paint(options.color_mode, BrandToken::Dim, right));
    output.push('\n');
}

pub fn render_prompt_input_box(terminal_width: u16, color_mode: ColorMode, label: &str) -> String {
    let box_width = prompt_input_box_width(terminal_width);
    let inner_width = box_width.saturating_sub(2);
    let label_width = visible_width(label);
    let prompt_padding = " ".repeat(inner_width.saturating_sub(label_width + 1));

    let mut output = String::new();
    output.push_str("  ");
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "┌"));
    output.push_str(&paint(
        color_mode,
        BrandToken::RaptorRed,
        &"─".repeat(inner_width),
    ));
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "┐"));
    output.push('\n');
    output.push_str("  ");
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "│"));
    output.push(' ');
    output.push_str(&style(color_mode, BrandToken::Accent, true, label));
    output.push_str(&prompt_padding);
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "│"));
    output.push('\n');
    output.push_str("  ");
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "└"));
    output.push_str(&paint(
        color_mode,
        BrandToken::RaptorRed,
        &"─".repeat(inner_width),
    ));
    output.push_str(&paint(color_mode, BrandToken::RaptorRed, "┘"));

    output
}

pub fn prompt_input_box_width(terminal_width: u16) -> usize {
    (terminal_width.saturating_sub(4) as usize).clamp(44, 120)
}

pub fn prompt_input_cursor_column(label: &str) -> u16 {
    (2 + 1 + 1 + visible_width(label)) as u16
}

fn icon_field(icon: &str) -> String {
    pad_visible(icon, 3)
}

fn fit_field(value: &str, width: usize) -> String {
    let value = fit_visible(value, width);
    pad_visible(&value, width)
}

fn fit_visible(value: &str, width: usize) -> String {
    if visible_width(value) <= width {
        return value.to_owned();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let mut tail = String::new();
    let mut tail_width = 0;
    for ch in value.chars().rev() {
        let ch_width = char_width(ch);
        if tail_width + ch_width > width - 3 {
            break;
        }
        tail.insert(0, ch);
        tail_width += ch_width;
    }
    format!("...{tail}")
}

fn pad_visible(value: &str, width: usize) -> String {
    let value_width = visible_width(value);
    if value_width >= width {
        return value.to_owned();
    }
    format!("{value}{}", " ".repeat(width - value_width))
}

fn visible_width(value: &str) -> usize {
    value.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    if ch.is_ascii()
        || matches!(
            ch,
            '─' | '│'
                | '╭'
                | '╮'
                | '╰'
                | '╯'
                | '┬'
                | '╔'
                | '╗'
                | '╚'
                | '╝'
                | '╠'
                | '╣'
                | '╦'
                | '╩'
                | '╬'
                | '═'
                | '║'
                | '⏱' // U+23F1 STOPWATCH — narrow (1 col) in terminals
        )
    {
        1
    } else {
        2
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

fn paint(mode: ColorMode, token: BrandToken, value: &str) -> String {
    style(mode, token, false, value)
}

fn hyperlink(mode: ColorMode, text: &str, url: &str) -> String {
    if mode == ColorMode::None {
        return text.to_owned();
    }
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}

fn style(mode: ColorMode, token: BrandToken, bold: bool, value: &str) -> String {
    match mode {
        ColorMode::None => value.to_owned(),
        ColorMode::Ansi16 => sgr(&[bold_code(bold), ansi16_code(token)], value),
        ColorMode::Ansi256 => sgr(&[bold_code(bold), "38", "5", ansi256_code(token)], value),
        ColorMode::Truecolor => {
            let (red, green, blue) = truecolor_rgb(token);
            let red = red.to_string();
            let green = green.to_string();
            let blue = blue.to_string();
            sgr(&[bold_code(bold), "38", "2", &red, &green, &blue], value)
        }
    }
}

fn sgr(codes: &[&str], value: &str) -> String {
    let codes = codes
        .iter()
        .copied()
        .filter(|code| !code.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    format!("\x1b[{codes}m{value}\x1b[0m")
}

fn bold_code(bold: bool) -> &'static str {
    if bold {
        "1"
    } else {
        ""
    }
}

fn ansi16_code(token: BrandToken) -> &'static str {
    match token {
        BrandToken::Accent => "97",
        BrandToken::Dim => "90",
        BrandToken::RaptorRed => "91",
        BrandToken::RaptorRedMuted => "31",
        BrandToken::Text => "37",
    }
}

fn ansi256_code(token: BrandToken) -> &'static str {
    match token {
        BrandToken::Accent => "15",
        BrandToken::Dim => "243",
        BrandToken::RaptorRed => "203",
        BrandToken::RaptorRedMuted => "88",
        BrandToken::Text => "188",
    }
}

fn truecolor_rgb(token: BrandToken) -> (u8, u8, u8) {
    let hex = match token {
        BrandToken::Accent => theme::ACCENT,
        BrandToken::Dim => theme::DIM,
        BrandToken::RaptorRed => theme::RAPTOR_RED,
        BrandToken::RaptorRedMuted => theme::RAPTOR_RED_MUTED,
        BrandToken::Text => theme::TEXT,
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
            emoji: true,
            interactive: true,
            mode: Some("high effort".to_owned()),
            model_name: Some("DeepSeek".to_owned()),
            terminal_width: 100,
            user_name: "developer".to_owned(),
            version: "0.1.0".to_owned(),
        }
    }

    fn snapshot_options(width: u16) -> StartupBannerOptions {
        StartupBannerOptions {
            api_key_url: "https://platform.deepseek.com/api_keys".to_owned(),
            ci: false,
            color_mode: ColorMode::Truecolor,
            compact: width < 80,
            cwd: PathBuf::from("/workspace/raptor-code"),
            deepseek_api_key_present: false,
            emoji: true,
            interactive: true,
            mode: Some("high effort".to_owned()),
            model_name: Some("DeepSeek".to_owned()),
            terminal_width: width,
            user_name: "developer".to_owned(),
            version: "0.1.0".to_owned(),
        }
    }

    fn snapshot_fixture(value: &'static str) -> String {
        // Normalize CRLF -> LF so the byte-for-byte compare holds on Windows,
        // where git may check the fixture out with CRLF line endings.
        let normalized = value.replace("\r\n", "\n");
        normalized
            .strip_suffix('\n')
            .unwrap_or(&normalized)
            .replace('␠', " ")
    }

    fn strip_ansi(value: &str) -> String {
        let mut output = String::with_capacity(value.len());
        let mut chars = value.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                match chars.peek() {
                    Some(&'[') => {
                        chars.next();
                        for ch in chars.by_ref() {
                            if ch == 'm' {
                                break;
                            }
                        }
                    }
                    Some(&']') => {
                        chars.next();
                        let mut prev = '\0';
                        for ch in chars.by_ref() {
                            if ch == '\x07' {
                                break;
                            }
                            if prev == '\x1b' && ch == '\\' {
                                break;
                            }
                            prev = ch;
                        }
                    }
                    _ => {}
                }
            } else {
                output.push(ch);
            }
        }

        output
    }

    #[test]
    fn wide_splash_snapshot_matches_fixture() {
        let output = strip_ansi(&render_startup_banner(&snapshot_options(120)));
        let expected = snapshot_fixture(include_str!("../tests/snapshots/splash_wide.txt"));

        assert_eq!(output, expected);
    }

    #[test]
    fn compact_splash_snapshot_matches_fixture() {
        let output = strip_ansi(&render_startup_banner(&snapshot_options(78)));
        let expected = snapshot_fixture(include_str!("../tests/snapshots/splash_compact.txt"));

        assert_eq!(output, expected);
    }

    #[test]
    fn banner_renders_required_brand_content() {
        let output = render_startup_banner(&startup_options());

        assert!(output.contains("Raptor Labs"));
        assert!(output.contains("raptorlabs.dev"));
        assert!(output.contains("│"));
        assert!(output.contains("██████╗"));
        assert!(output.contains("Raptor Code"));
        assert!(output.contains("v0.1"));
        assert!(output.contains("agent runtime ready"));
        assert!(output.contains("Agentic software engineering at predator speed."));
        assert!(output.contains("Welcome, developer."));
        assert!(output.contains("Agents standing by."));
        assert!(output.contains("Context scanned · toolchain ready · awaiting your command."));
        assert!(output.contains("Working directory"));
        assert!(output.contains("DeepSeek"));
        assert!(output.contains("high effort"));
        assert!(output.contains("Type"));
        assert!(output.contains("/help for commands"));
        assert!(!output.contains("raptorcode>"));
        assert!(!output.ends_with('\n'));
        assert!(!output.contains("raptor>"));
        assert!(!output.contains("DeepSeek API key"));
        assert!(!output.contains("https://platform.deepseek.com/api_keys"));
    }

    #[test]
    fn no_color_banner_has_no_ansi_codes() {
        let output = render_startup_banner(&startup_options());

        assert!(!output.contains("\x1b["));
    }

    #[test]
    fn no_color_environment_selects_monochrome_output() {
        let previous_no_color = env::var_os("NO_COLOR");
        env::set_var("NO_COLOR", "1");

        let mut options = startup_options();
        options.color_mode = color_mode_from_environment(true, false);
        let output = render_startup_banner(&options);

        if let Some(value) = previous_no_color {
            env::set_var("NO_COLOR", value);
        } else {
            env::remove_var("NO_COLOR");
        }

        assert!(!output.contains("\x1b["));
    }

    #[test]
    fn truecolor_banner_uses_ansi_codes() {
        let mut options = startup_options();
        options.color_mode = ColorMode::Truecolor;

        let output = render_startup_banner(&options);

        assert!(output.contains("\x1b[1;38;2;230;57;70m"));
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

        assert!(output.contains("RAPTOR CODE"));
        assert!(output.contains("Raptor Labs"));
        assert!(!output.contains("████"));
        assert!(output.contains("Raptor Code"));
        assert!(output.contains("v0.1"));
        assert!(!output.contains("raptorcode>"));
        assert!(!output.contains("raptor>"));
    }

    #[test]
    fn prompt_input_box_wraps_live_input_label() {
        let output = render_prompt_input_box(100, ColorMode::None, ">  ");

        assert!(output.contains("┌"));
        assert!(output.contains("┐"));
        assert!(output.contains("│ >"));
        assert!(output.contains("└"));
        assert!(output.contains("┘"));
        assert_eq!(prompt_input_cursor_column(">  "), 7);
    }

    #[test]
    fn startup_banner_matches_premium_metadata_shape() {
        let output = render_startup_banner(&startup_options());

        assert!(output.contains("╭"));
        assert!(output.contains("╮"));
        assert!(output.contains("╰"));
        assert!(output.contains("╯"));
        assert!(output.contains("📁"));
        assert!(output.contains("🔗"));
        assert!(output.contains("⏱"));
        assert!(output.contains(">_"));
        assert!(output.contains("Type"));
    }

    #[test]
    fn no_emoji_mode_uses_ascii_fallback_icons() {
        let mut options = startup_options();
        options.emoji = false;

        let output = render_startup_banner(&options);

        assert!(output.contains("[/\\]"));
        assert!(output.contains("[]"));
        assert!(output.contains("<>"));
        assert!(output.contains("::"));
        assert!(!output.contains("🦷"));
        assert!(!output.contains("📁"));
        assert!(!output.contains("🔗"));
        assert!(!output.contains("⏱"));
    }

    #[test]
    fn detected_key_state_does_not_print_key_value() {
        let mut options = startup_options();
        options.deepseek_api_key_present = true;

        let output = render_startup_banner(&options);

        assert!(!output.contains("dummy"));
        assert!(!output.contains("sk-"));
        assert!(!output.contains("https://platform.deepseek.com/api_keys"));
        assert!(!output.contains("DeepSeek API key"));
    }

    #[test]
    fn ci_banner_is_minimal_and_script_safe() {
        let mut options = startup_options();
        options.ci = true;

        let output = render_startup_banner(&options);

        assert_eq!(output, "Raptor Code v0.1 - Raptor Labs\n");
        assert!(!output.contains("\x1b["));
        assert!(!output.contains("raptorcode>"));
    }

    #[test]
    fn dashboard_snapshot_renders_core_state() {
        let mut state = DashboardState::empty("0.1.0", "/workspace/project");
        state.deepseek_api_key_present = true;
        state.agent_roles[0].status = "planning_completed".to_owned();

        let output = render_dashboard_snapshot(&state);

        assert!(output.contains("Raptor Code v0.1"));
        assert!(output.contains("Raptor Labs"));
        assert!(output.contains("DeepSeek / DeepSeek V4 Pro"));
        assert!(output.contains("frontend-agent"));
        assert!(output.contains("q to quit"));
    }
}
