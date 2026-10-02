use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_RECENT_ACTIVITY: usize = 64;
/// How long each footer quote stays on screen before the dashboard rotates to the
/// next one in the pool.
const QUOTE_ROTATION_INTERVAL: Duration = Duration::from_secs(8);

/// Crimson accents, dim chrome, white emphasis — the RaptorCode terminal
/// palette. The RGB values are owned by `crate::theme` alongside the banner
/// colors so the brand palette has a single home.
const RAPTOR_RED: Color = Color::Rgb(
    crate::theme::LIVE_RED_RGB.0,
    crate::theme::LIVE_RED_RGB.1,
    crate::theme::LIVE_RED_RGB.2,
);
const RAPTOR_DIM: Color = Color::Rgb(
    crate::theme::LIVE_DIM_RGB.0,
    crate::theme::LIVE_DIM_RGB.1,
    crate::theme::LIVE_DIM_RGB.2,
);
const RAPTOR_TRACK: Color = Color::Rgb(
    crate::theme::LIVE_TRACK_RGB.0,
    crate::theme::LIVE_TRACK_RGB.1,
    crate::theme::LIVE_TRACK_RGB.2,
);

/// Events emitted by live swarm agent workers and consumed by the dashboard.
#[derive(Debug)]
pub enum SwarmUiEvent {
    AgentSpawned {
        agent: usize,
    },
    AgentStatus {
        agent: usize,
        line: String,
        detail: Option<String>,
    },
    AgentStep {
        agent: usize,
        step: u32,
    },
    AgentFileWritten {
        agent: usize,
        path: String,
    },
    AgentNeedsConfirmation {
        agent: usize,
        message: String,
        reply: Sender<bool>,
    },
    AgentUsage {
        agent: usize,
        total_tokens: u64,
    },
    AgentFinished {
        agent: usize,
        summary: String,
    },
    AgentFailed {
        agent: usize,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwarmAgentPhase {
    Pending,
    Working,
    AwaitingApproval,
    Done,
    Failed,
}

impl SwarmAgentPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, SwarmAgentPhase::Done | SwarmAgentPhase::Failed)
    }

    fn label(self) -> &'static str {
        match self {
            SwarmAgentPhase::Pending => "spawning",
            SwarmAgentPhase::Working => "working",
            SwarmAgentPhase::AwaitingApproval => "awaiting approval",
            SwarmAgentPhase::Done => "done",
            SwarmAgentPhase::Failed => "stopped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SwarmAgentUiState {
    pub detail: Option<String>,
    pub files_written: usize,
    pub phase: SwarmAgentPhase,
    pub role: String,
    pub status_line: String,
    pub steps: u32,
    pub summary: Option<String>,
}

impl SwarmAgentUiState {
    pub fn pending(role: impl Into<String>) -> Self {
        Self {
            detail: None,
            files_written: 0,
            phase: SwarmAgentPhase::Pending,
            role: role.into(),
            status_line: "Preparing isolated worktree".to_owned(),
            steps: 0,
            summary: None,
        }
    }
}

struct PendingConfirmation {
    agent: usize,
    message: String,
    reply: Sender<bool>,
}

pub struct SwarmDashboardState {
    pub agents: Vec<SwarmAgentUiState>,
    pub focused: Option<usize>,
    pub max_steps: u32,
    pub mission: String,
    pub model_label: String,
    /// The footer quote currently on screen. Sourced from `quotes` and advanced
    /// during the run by [`SwarmDashboardState::rotate_quote`].
    pub quote: String,
    pub recent_activity: VecDeque<String>,
    pub stopping: bool,
    pub total_tokens: u64,
    confirmations: VecDeque<PendingConfirmation>,
    /// Pool of footer quotes to cycle through while the swarm runs. The caller
    /// supplies the pool (and its starting order) so this crate stays free of the
    /// embedded quote asset.
    quotes: Vec<String>,
    quote_cursor: usize,
}

impl SwarmDashboardState {
    pub fn new(
        mission: impl Into<String>,
        model_label: impl Into<String>,
        roles: &[String],
        max_steps: u32,
        quotes: Vec<String>,
    ) -> Self {
        let quote = quotes.first().cloned().unwrap_or_default();
        Self {
            agents: roles
                .iter()
                .map(|role| SwarmAgentUiState::pending(role.clone()))
                .collect(),
            focused: None,
            max_steps,
            mission: mission.into(),
            model_label: model_label.into(),
            quote,
            recent_activity: VecDeque::new(),
            stopping: false,
            total_tokens: 0,
            confirmations: VecDeque::new(),
            quotes,
            quote_cursor: 0,
        }
    }

    /// Advances the footer to the next quote in the pool, wrapping around. A
    /// no-op when the pool has fewer than two quotes (nothing to rotate to).
    pub fn rotate_quote(&mut self) {
        if self.quotes.len() < 2 {
            return;
        }
        self.quote_cursor = (self.quote_cursor + 1) % self.quotes.len();
        self.quote = self.quotes[self.quote_cursor].clone();
    }

    pub fn all_agents_terminal(&self) -> bool {
        !self.agents.is_empty() && self.agents.iter().all(|agent| agent.phase.is_terminal())
    }

    pub fn has_pending_confirmation(&self) -> bool {
        !self.confirmations.is_empty()
    }

    fn push_activity(&mut self, agent: usize, line: &str) {
        let role = self
            .agents
            .get(agent)
            .map(|a| short_role(&a.role))
            .unwrap_or("agent");
        self.recent_activity.push_back(format!("{role:<9} {line}"));
        while self.recent_activity.len() > MAX_RECENT_ACTIVITY {
            self.recent_activity.pop_front();
        }
    }

    fn answer_front_confirmation(&mut self, allow: bool) {
        if let Some(pending) = self.confirmations.pop_front() {
            let _ = pending.reply.send(allow);
            if let Some(agent) = self.agents.get_mut(pending.agent) {
                if !agent.phase.is_terminal() {
                    agent.phase = SwarmAgentPhase::Working;
                }
            }
            let verdict = if allow { "approved" } else { "denied" };
            self.push_activity(pending.agent, &format!("policy {verdict}"));
        }
    }

    fn deny_all_confirmations(&mut self) {
        while !self.confirmations.is_empty() {
            self.answer_front_confirmation(false);
        }
    }
}

/// Applies one worker event to the dashboard state. Pure with respect to the
/// terminal: rendering happens separately, so this is unit-testable.
pub fn apply_swarm_event(state: &mut SwarmDashboardState, event: SwarmUiEvent) {
    match event {
        SwarmUiEvent::AgentSpawned { agent } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.phase = SwarmAgentPhase::Working;
                entry.status_line = "Reading the mission".to_owned();
            }
            state.push_activity(agent, "spawned");
        }
        SwarmUiEvent::AgentStatus {
            agent,
            line,
            detail,
        } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.status_line = line.clone();
                entry.detail = detail;
            }
            state.push_activity(agent, &line);
        }
        SwarmUiEvent::AgentStep { agent, step } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.steps = step;
            }
        }
        SwarmUiEvent::AgentFileWritten { agent, path } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.files_written += 1;
                entry.detail = Some(path.clone());
            }
            state.push_activity(agent, &format!("✓ wrote {path}"));
        }
        SwarmUiEvent::AgentNeedsConfirmation {
            agent,
            message,
            reply,
        } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.phase = SwarmAgentPhase::AwaitingApproval;
            }
            state.push_activity(agent, "⚠ waiting for developer approval");
            state.confirmations.push_back(PendingConfirmation {
                agent,
                message,
                reply,
            });
        }
        SwarmUiEvent::AgentUsage {
            agent: _,
            total_tokens,
        } => {
            // Cumulative spend, not context size: each event carries one API
            // step's total_tokens (whose prompt side re-counts the growing
            // history), so the footer figure is billed tokens across all
            // agents and steps — intentionally mirroring what the run costs.
            state.total_tokens = state.total_tokens.saturating_add(total_tokens);
        }
        SwarmUiEvent::AgentFinished { agent, summary } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.phase = SwarmAgentPhase::Done;
                entry.status_line = "Task complete".to_owned();
                entry.detail = None;
                entry.summary = Some(summary);
            }
            state.push_activity(agent, "✓ finished");
        }
        SwarmUiEvent::AgentFailed { agent, error } => {
            if let Some(entry) = state.agents.get_mut(agent) {
                entry.phase = SwarmAgentPhase::Failed;
                entry.status_line = error.clone();
                entry.detail = None;
            }
            state.push_activity(agent, &format!("✗ {error}"));
        }
    }
}

/// Runs the live swarm dashboard until every agent reaches a terminal phase
/// and the worker event channel disconnects. Returns `true` when the
/// developer requested an abort with `q`/`Esc`.
pub fn run_swarm_dashboard(
    mut state: SwarmDashboardState,
    events: Receiver<SwarmUiEvent>,
    cancel: Arc<AtomicBool>,
) -> io::Result<bool> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_swarm_dashboard_loop(&mut terminal, &mut state, &events, &cancel);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_swarm_dashboard_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    state: &mut SwarmDashboardState,
    events: &Receiver<SwarmUiEvent>,
    cancel: &Arc<AtomicBool>,
) -> io::Result<bool> {
    let started_at = Instant::now();
    let mut workers_disconnected = false;
    let mut last_quote_rotation = Instant::now();

    loop {
        loop {
            match events.try_recv() {
                Ok(event) => apply_swarm_event(state, event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    workers_disconnected = true;
                    break;
                }
            }
        }

        // Cycle the footer quote periodically so a long run shows variety rather
        // than a single quote frozen for its whole duration.
        if last_quote_rotation.elapsed() >= QUOTE_ROTATION_INTERVAL {
            state.rotate_quote();
            last_quote_rotation = Instant::now();
        }

        let elapsed = started_at.elapsed();
        terminal.draw(|frame| draw_swarm_dashboard(frame, state, elapsed))?;

        let finished = workers_disconnected && state.all_agents_terminal();

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if finished {
                    return Ok(state.stopping);
                }
                if state.has_pending_confirmation() {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            state.answer_front_confirmation(true)
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') => {
                            state.answer_front_confirmation(false)
                        }
                        KeyCode::Char('q') | KeyCode::Esc => {
                            request_stop(state, cancel);
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => request_stop(state, cancel),
                    KeyCode::Char(digit @ '1'..='4') => {
                        let index = digit as usize - '1' as usize;
                        if index < state.agents.len() {
                            state.focused = if state.focused == Some(index) {
                                None
                            } else {
                                Some(index)
                            };
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn request_stop(state: &mut SwarmDashboardState, cancel: &Arc<AtomicBool>) {
    cancel.store(true, Ordering::Relaxed);
    state.stopping = true;
    state.deny_all_confirmations();
}

fn draw_swarm_dashboard(frame: &mut Frame<'_>, state: &SwarmDashboardState, elapsed: Duration) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .horizontal_margin(2)
        .vertical_margin(1)
        .constraints([
            Constraint::Length(1), // header bar
            Constraint::Length(1), // dim divider
            Constraint::Length(1), // command line
            Constraint::Length(1), // bright red rule
            Constraint::Min(8),    // 2x2 agent grid
            Constraint::Length(1), // dim divider
            Constraint::Length(1), // footer bar
        ])
        .split(area);

    draw_header_bar(frame, chunks[0]);
    frame.render_widget(
        rule_line(chunks[1].width as usize, "─", RAPTOR_DIM),
        chunks[1],
    );
    draw_command_line(frame, state, chunks[2]);
    frame.render_widget(
        rule_line(chunks[3].width as usize, "─", RAPTOR_RED),
        chunks[3],
    );
    draw_agent_grid(frame, state, elapsed, chunks[4]);
    frame.render_widget(
        rule_line(chunks[5].width as usize, "─", RAPTOR_DIM),
        chunks[5],
    );
    draw_footer_bar(frame, state, elapsed, chunks[6]);

    if let Some(pending) = state.confirmations.front() {
        draw_confirmation_modal(frame, state, pending, area);
    }
}

fn draw_header_bar(frame: &mut Frame<'_>, area: Rect) {
    let width = area.width as usize;
    let left = vec![
        Span::styled("■ ", Style::default().fg(RAPTOR_RED)),
        Span::styled(
            "raptor — swarm orchestrator",
            Style::default().fg(RAPTOR_DIM),
        ),
    ];
    let right = vec![Span::styled(
        "ULTRATHINK · EFFORT=MAX",
        Style::default().fg(RAPTOR_DIM).add_modifier(Modifier::BOLD),
    )];
    frame.render_widget(Paragraph::new(padded_line(left, right, width)), area);
}

fn draw_command_line(frame: &mut Frame<'_>, state: &SwarmDashboardState, area: Rect) {
    let mission = truncate_line(&state.mission, area.width.saturating_sub(24) as usize);
    let line = Line::from(vec![
        Span::styled(
            "raptor",
            Style::default().fg(RAPTOR_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ❯ ", Style::default().fg(RAPTOR_DIM)),
        Span::styled(
            "swarm run ",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("\"{mission}\""), Style::default().fg(Color::White)),
        Span::styled(" █", Style::default().fg(RAPTOR_RED)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_agent_grid(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    elapsed: Duration,
    area: Rect,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    draw_agent_row(frame, state, elapsed, rows[0], 0);
    frame.render_widget(rule_line(area.width as usize, "─", RAPTOR_RED), rows[1]);
    draw_agent_row(frame, state, elapsed, rows[2], 2);
}

fn draw_agent_row(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    elapsed: Duration,
    area: Rect,
    first_index: usize,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    for (offset, column) in columns.iter().enumerate() {
        let index = first_index + offset;
        if let Some(agent) = state.agents.get(index) {
            let focused = state.focused == Some(index);
            draw_agent_panel(frame, state, agent, elapsed, *column, focused);
        }
    }
}

fn draw_agent_panel(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    agent: &SwarmAgentUiState,
    elapsed: Duration,
    area: Rect,
    focused: bool,
) {
    // Pad each cell so the two columns read as separated, borderless tiles.
    let inner = Rect {
        x: area.x + 1,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };
    if inner.width < 6 || inner.height < 3 {
        return;
    }
    let width = inner.width as usize;

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // role header
            Constraint::Min(1),    // current file + status (vertically centered)
            Constraint::Length(1), // counts + progress bar
        ])
        .split(inner);

    // --- header: indicator + ROLE ............ .wt/role ---
    let (indicator, indicator_color) = match agent.phase {
        SwarmAgentPhase::Working | SwarmAgentPhase::Pending => {
            (spinner_frame(elapsed).to_owned(), RAPTOR_RED)
        }
        SwarmAgentPhase::AwaitingApproval => ("⚠".to_owned(), Color::Yellow),
        SwarmAgentPhase::Done => ("✓".to_owned(), Color::Green),
        SwarmAgentPhase::Failed => ("✗".to_owned(), RAPTOR_RED),
    };
    // Focused panels (keys 1-4) lead with a crimson marker and tint the role
    // name so the selected specialist stands out at a glance.
    let role_color = if focused { RAPTOR_RED } else { Color::White };
    let mut header_left = Vec::with_capacity(3);
    if focused {
        header_left.push(Span::styled("▸ ", Style::default().fg(RAPTOR_RED)));
    }
    header_left.push(Span::styled(
        format!("{indicator} "),
        Style::default().fg(indicator_color),
    ));
    header_left.push(Span::styled(
        short_role(&agent.role).to_uppercase(),
        Style::default().fg(role_color).add_modifier(Modifier::BOLD),
    ));
    let header_right = vec![Span::styled(
        format!(".wt/{}", short_role(&agent.role)),
        Style::default().fg(RAPTOR_DIM),
    )];
    frame.render_widget(
        Paragraph::new(padded_line(header_left, header_right, width)),
        parts[0],
    );

    // --- middle: current file (bold) + status (dim), vertically centered ---
    let file = agent
        .detail
        .as_deref()
        .map(|detail| truncate_line(detail, width))
        .unwrap_or_default();
    let status = truncate_line(&agent.status_line, width);
    let mid_height = parts[1].height as usize;
    let pad_top = mid_height.saturating_sub(2) / 2;
    let mut mid_lines: Vec<Line<'_>> = Vec::with_capacity(mid_height);
    for _ in 0..pad_top {
        mid_lines.push(Line::from(""));
    }
    mid_lines.push(Line::from(Span::styled(
        file,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));
    mid_lines.push(Line::from(Span::styled(
        status,
        Style::default().fg(RAPTOR_DIM),
    )));
    frame.render_widget(
        Paragraph::new(mid_lines).alignment(Alignment::Center),
        parts[1],
    );

    // --- footer: "N files  M steps" + progress bar ---
    let mut counts = vec![
        Span::styled(
            agent.files_written.to_string(),
            Style::default().fg(RAPTOR_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" files  ", Style::default().fg(RAPTOR_DIM)),
        Span::styled(
            agent.steps.to_string(),
            Style::default().fg(RAPTOR_RED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" steps  ", Style::default().fg(RAPTOR_DIM)),
    ];
    let used = spans_width(&counts);
    let bar_width = width.saturating_sub(used);
    let fraction = if state.max_steps == 0 {
        0.0
    } else {
        (agent.steps as f64 / state.max_steps as f64).clamp(0.0, 1.0)
    };
    counts.extend(progress_bar_spans(fraction, bar_width));
    frame.render_widget(Paragraph::new(Line::from(counts)), parts[2]);
}

fn draw_footer_bar(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    elapsed: Duration,
    area: Rect,
) {
    let width = area.width as usize;
    let active = state
        .agents
        .iter()
        .filter(|agent| !agent.phase.is_terminal())
        .count();

    let (dot_color, status_text, status_color) = if state.all_agents_terminal() {
        (Color::Green, "swarm complete".to_owned(), Color::Green)
    } else if state.stopping {
        (
            Color::Yellow,
            format!("{active} agents live · stopping"),
            Color::Yellow,
        )
    } else {
        (RAPTOR_RED, format!("{active} agents live"), RAPTOR_RED)
    };

    let left = vec![
        Span::styled("● ", Style::default().fg(dot_color)),
        Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   elapsed {}", format_elapsed(elapsed)),
            Style::default().fg(RAPTOR_DIM),
        ),
        Span::styled(
            format!("   tokens {}", format_tokens(state.total_tokens)),
            Style::default().fg(RAPTOR_DIM),
        ),
    ];

    let quote_budget = width.saturating_sub(spans_width(&left) + 3);
    let right = if quote_budget >= 12 && !state.quote.is_empty() {
        vec![Span::styled(
            format!(
                "\"{}\"",
                truncate_line(&state.quote, quote_budget.saturating_sub(2))
            ),
            Style::default()
                .fg(RAPTOR_RED)
                .add_modifier(Modifier::ITALIC),
        )]
    } else {
        Vec::new()
    };

    frame.render_widget(Paragraph::new(padded_line(left, right, width)), area);
}

/// Builds a left-aligned, right-aligned single line: `left … right`, padding the
/// gap so `right` hugs the far edge of `width`.
fn padded_line(
    mut left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: usize,
) -> Line<'static> {
    let used = spans_width(&left) + spans_width(&right);
    let pad = width.saturating_sub(used).max(1);
    left.push(Span::raw(" ".repeat(pad)));
    left.extend(right);
    Line::from(left)
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn rule_line(width: usize, ch: &str, color: Color) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        ch.repeat(width),
        Style::default().fg(color),
    )))
}

fn progress_bar_spans(fraction: f64, width: usize) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let filled = ((fraction * width as f64).round() as usize).min(width);
    vec![
        Span::styled("━".repeat(filled), Style::default().fg(RAPTOR_RED)),
        Span::styled(
            "─".repeat(width - filled),
            Style::default().fg(RAPTOR_TRACK),
        ),
    ]
}

fn draw_confirmation_modal(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    pending: &PendingConfirmation,
    area: Rect,
) {
    let role = state
        .agents
        .get(pending.agent)
        .map(|agent| short_role(&agent.role))
        .unwrap_or("agent");
    let modal_area = centered_rect(70, 8, area);
    frame.render_widget(Clear, modal_area);
    let modal = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("{role} agent requests approval:"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            pending.message.clone(),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "[y] allow · [n] deny",
            Style::default().fg(Color::Gray),
        )),
    ])
    .wrap(Wrap { trim: true })
    .block(
        Block::default()
            .title(" ⚠ Policy check ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    frame.render_widget(modal, modal_area);
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let width = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height: height.min(area.height),
    }
}

/// Plain-text snapshot of the swarm dashboard for tests and non-TTY output.
pub fn render_swarm_snapshot(state: &SwarmDashboardState) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "RaptorCode Swarm — {} · {}\n",
        state.model_label, state.mission
    ));
    for agent in &state.agents {
        output.push_str(&format!(
            "[{}] {} — {} ({} files, step {}/{})\n",
            agent.phase.label(),
            short_role(&agent.role),
            agent.status_line,
            agent.files_written,
            agent.steps,
            state.max_steps
        ));
    }
    output.push_str(&format!("tokens: {}\n", format_tokens(state.total_tokens)));
    output
}

fn spinner_frame(elapsed: Duration) -> &'static str {
    let index = (elapsed.as_millis() / 120) as usize % SPINNER_FRAMES.len();
    SPINNER_FRAMES[index]
}

fn short_role(role: &str) -> &str {
    role.strip_suffix("-agent").unwrap_or(role)
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = line.chars().count();
    if count <= max_chars {
        return line.to_owned();
    }
    let truncated: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn format_tokens(total: u64) -> String {
    if total >= 1_000_000 {
        format!("{:.1}m", total as f64 / 1_000_000.0)
    } else if total >= 1_000 {
        format!("{:.1}k", total as f64 / 1_000.0)
    } else {
        total.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn test_state() -> SwarmDashboardState {
        let roles = vec![
            "frontend-agent".to_owned(),
            "backend-agent".to_owned(),
            "test-agent".to_owned(),
            "security-agent".to_owned(),
        ];
        SwarmDashboardState::new(
            "Build a SaaS app",
            "DeepSeek V4 Pro",
            &roles,
            100,
            vec!["the quieter you become, the more you are able to hear".to_owned()],
        )
    }

    #[test]
    fn rotate_quote_cycles_through_the_pool_and_wraps() {
        let roles = vec!["frontend-agent".to_owned()];
        let mut state = SwarmDashboardState::new(
            "mission",
            "model",
            &roles,
            100,
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
        );

        assert_eq!(state.quote, "first");
        state.rotate_quote();
        assert_eq!(state.quote, "second");
        state.rotate_quote();
        assert_eq!(state.quote, "third");
        state.rotate_quote();
        assert_eq!(state.quote, "first", "rotation wraps back to the start");
    }

    #[test]
    fn rotate_quote_is_a_noop_for_a_single_quote_pool() {
        let mut state = test_state();
        let original = state.quote.clone();
        state.rotate_quote();
        assert_eq!(state.quote, original);
    }

    #[test]
    fn spawn_status_and_finish_events_update_agent_state() {
        let mut state = test_state();

        apply_swarm_event(&mut state, SwarmUiEvent::AgentSpawned { agent: 0 });
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentStatus {
                agent: 0,
                line: "Building pricing component".to_owned(),
                detail: Some("src/Pricing.tsx".to_owned()),
            },
        );
        apply_swarm_event(&mut state, SwarmUiEvent::AgentStep { agent: 0, step: 7 });
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentFileWritten {
                agent: 0,
                path: "src/Pricing.tsx".to_owned(),
            },
        );
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentFinished {
                agent: 0,
                summary: "Pricing page complete".to_owned(),
            },
        );

        let agent = &state.agents[0];
        assert_eq!(agent.phase, SwarmAgentPhase::Done);
        assert_eq!(agent.files_written, 1);
        assert_eq!(agent.steps, 7);
        assert_eq!(agent.summary.as_deref(), Some("Pricing page complete"));
        assert!(!state.all_agents_terminal());
    }

    #[test]
    fn confirmation_events_queue_and_answer_through_reply_channel() {
        let mut state = test_state();
        let (reply_tx, reply_rx) = channel();

        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentNeedsConfirmation {
                agent: 1,
                message: "Force delete /tmp/x?".to_owned(),
                reply: reply_tx,
            },
        );

        assert!(state.has_pending_confirmation());
        assert_eq!(state.agents[1].phase, SwarmAgentPhase::AwaitingApproval);

        state.answer_front_confirmation(true);

        assert!(!state.has_pending_confirmation());
        assert!(reply_rx.recv().unwrap());
        assert_eq!(state.agents[1].phase, SwarmAgentPhase::Working);
    }

    #[test]
    fn failed_and_finished_agents_make_swarm_terminal() {
        let mut state = test_state();
        for agent in 0..3 {
            apply_swarm_event(
                &mut state,
                SwarmUiEvent::AgentFinished {
                    agent,
                    summary: "done".to_owned(),
                },
            );
        }
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentFailed {
                agent: 3,
                error: "aborted".to_owned(),
            },
        );

        assert!(state.all_agents_terminal());
        let snapshot = render_swarm_snapshot(&state);
        assert!(snapshot.contains("[done] frontend"));
        assert!(snapshot.contains("[stopped] security"));
    }

    #[test]
    fn focused_agent_panel_renders_focus_marker() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut state = test_state();
        for agent in 0..4 {
            apply_swarm_event(&mut state, SwarmUiEvent::AgentSpawned { agent });
        }
        state.focused = Some(2);

        let backend = TestBackend::new(92, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_swarm_dashboard(frame, &state, Duration::from_secs(5)))
            .unwrap();
        let rendered = buffer_rows(terminal.backend()).join("\n");

        let marker_line = rendered
            .lines()
            .find(|line| line.contains('▸'))
            .expect("focused panel must render the ▸ marker");
        assert!(
            marker_line.contains("TEST"),
            "marker must sit on the focused TEST panel's header: {marker_line}"
        );
        assert_eq!(rendered.matches('▸').count(), 1);
    }

    fn buffer_rows(backend: &ratatui::backend::TestBackend) -> Vec<String> {
        let buffer = backend.buffer();
        let area = *buffer.area();
        (0..area.height)
            .map(|y| {
                let mut row = String::new();
                for x in 0..area.width {
                    row.push_str(buffer[(x, y)].symbol());
                }
                row.trim_end().to_owned()
            })
            .collect()
    }

    #[test]
    fn dashboard_renders_screenshot_layout() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut state = test_state();
        // Drive the four agents into a working state resembling the reference UI.
        let scenario = [
            (
                0usize,
                "writing payment element + card validation",
                "src/checkout/StripeForm.tsx",
                12u32,
                4usize,
            ),
            (
                1,
                "wiring PaymentIntent + idempotency keys",
                "api/payments/create_intent.rs",
                18,
                6,
            ),
            (
                2,
                "covering decline + 3DS challenge paths",
                "tests/checkout_flow.spec.ts",
                9,
                3,
            ),
            (
                3,
                "verifying webhook signatures, scrubbing logs",
                "api/payments/webhook.rs",
                7,
                2,
            ),
        ];
        for (agent, status, file, steps, files) in scenario {
            apply_swarm_event(&mut state, SwarmUiEvent::AgentSpawned { agent });
            apply_swarm_event(
                &mut state,
                SwarmUiEvent::AgentStatus {
                    agent,
                    line: status.to_owned(),
                    detail: Some(file.to_owned()),
                },
            );
            apply_swarm_event(&mut state, SwarmUiEvent::AgentStep { agent, step: steps });
            for _ in 0..files {
                apply_swarm_event(
                    &mut state,
                    SwarmUiEvent::AgentFileWritten {
                        agent,
                        path: file.to_owned(),
                    },
                );
            }
        }
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentUsage {
                agent: 0,
                total_tokens: 318_400,
            },
        );

        let backend = TestBackend::new(92, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_swarm_dashboard(frame, &state, Duration::from_secs(49)))
            .unwrap();
        let rows = buffer_rows(terminal.backend());
        let rendered = rows.join("\n");
        eprintln!("\n{rendered}\n");

        for expected in [
            "raptor — swarm orchestrator",
            "ULTRATHINK · EFFORT=MAX",
            "swarm run",
            "FRONTEND",
            ".wt/frontend",
            "BACKEND",
            ".wt/backend",
            "src/checkout/StripeForm.tsx",
            "writing payment element + card validation",
            "files",
            "steps",
            "agents live",
            "elapsed 00:49",
            "tokens 318.4k",
            "the quieter you become",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected:?} in:\n{rendered}"
            );
        }
    }

    #[test]
    fn usage_events_accumulate_tokens() {
        let mut state = test_state();
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentUsage {
                agent: 0,
                total_tokens: 1_500,
            },
        );
        apply_swarm_event(
            &mut state,
            SwarmUiEvent::AgentUsage {
                agent: 1,
                total_tokens: 700,
            },
        );

        assert_eq!(state.total_tokens, 2_200);
        assert_eq!(format_tokens(state.total_tokens), "2.2k");
    }
}
