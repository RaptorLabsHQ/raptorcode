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
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const MAX_RECENT_ACTIVITY: usize = 64;
const ACTIVITY_PANEL_LINES: usize = 4;

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

    fn border_color(self) -> Color {
        match self {
            SwarmAgentPhase::Pending => Color::DarkGray,
            SwarmAgentPhase::Working => Color::Red,
            SwarmAgentPhase::AwaitingApproval => Color::Yellow,
            SwarmAgentPhase::Done => Color::Green,
            SwarmAgentPhase::Failed => Color::LightRed,
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
    pub recent_activity: VecDeque<String>,
    pub stopping: bool,
    pub total_tokens: u64,
    confirmations: VecDeque<PendingConfirmation>,
}

impl SwarmDashboardState {
    pub fn new(
        mission: impl Into<String>,
        model_label: impl Into<String>,
        roles: &[String],
        max_steps: u32,
    ) -> Self {
        Self {
            agents: roles
                .iter()
                .map(|role| SwarmAgentUiState::pending(role.clone()))
                .collect(),
            focused: None,
            max_steps,
            mission: mission.into(),
            model_label: model_label.into(),
            recent_activity: VecDeque::new(),
            stopping: false,
            total_tokens: 0,
            confirmations: VecDeque::new(),
        }
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
    let area = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(12),
            Constraint::Length(ACTIVITY_PANEL_LINES as u16 + 2),
            Constraint::Length(1),
        ])
        .split(area);

    draw_swarm_header(frame, state, elapsed, chunks[0]);
    draw_agent_grid(frame, state, elapsed, chunks[1]);
    draw_activity_feed(frame, state, chunks[2]);
    draw_footer(frame, state, chunks[3]);

    if let Some(pending) = state.confirmations.front() {
        draw_confirmation_modal(frame, state, pending, area);
    }
}

fn draw_swarm_header(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    elapsed: Duration,
    area: Rect,
) {
    let active = state
        .agents
        .iter()
        .filter(|agent| !agent.phase.is_terminal())
        .count();
    let status = if state.stopping {
        " · stopping".to_owned()
    } else {
        String::new()
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " ▟█▙ RAPTORCODE SWARM ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    " {} · {} specialists active · {}{}",
                    state.model_label,
                    active,
                    format_elapsed(elapsed),
                    status
                ),
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Mission  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_line(&state.mission, 110),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::Red)),
    );
    frame.render_widget(header, area);
}

fn draw_agent_grid(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    elapsed: Duration,
    area: Rect,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    for (row_index, row) in rows.iter().enumerate() {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(*row);
        for (col_index, column) in columns.iter().enumerate() {
            let agent_index = row_index * 2 + col_index;
            if let Some(agent) = state.agents.get(agent_index) {
                draw_agent_panel(frame, state, agent, agent_index, elapsed, *column);
            }
        }
    }
}

fn draw_agent_panel(
    frame: &mut Frame<'_>,
    state: &SwarmDashboardState,
    agent: &SwarmAgentUiState,
    agent_index: usize,
    elapsed: Duration,
    area: Rect,
) {
    let phase_indicator = match agent.phase {
        SwarmAgentPhase::Working | SwarmAgentPhase::Pending => spinner_frame(elapsed).to_owned(),
        SwarmAgentPhase::AwaitingApproval => "⚠".to_owned(),
        SwarmAgentPhase::Done => "✓".to_owned(),
        SwarmAgentPhase::Failed => "✗".to_owned(),
    };
    let title = format!(
        " ◢ {} — {} {} ",
        short_role(&agent.role).to_uppercase(),
        agent.phase.label(),
        phase_indicator
    );
    let border_color = if state.focused == Some(agent_index) {
        Color::White
    } else {
        agent.phase.border_color()
    };

    let mut lines = vec![
        Line::from(Span::styled(
            truncate_line(&agent.status_line, area.width.saturating_sub(4) as usize),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            agent
                .detail
                .as_deref()
                .map(|detail| {
                    format!(
                        "└─ {}",
                        truncate_line(detail, area.width.saturating_sub(7) as usize)
                    )
                })
                .unwrap_or_default(),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "{} files · step {}/{}",
                agent.files_written, agent.steps, state.max_steps
            ),
            Style::default().fg(Color::Gray),
        )),
    ];

    if let Some(summary) = agent.summary.as_deref() {
        lines.push(Line::from(Span::styled(
            truncate_line(summary, area.width.saturating_sub(4) as usize),
            Style::default().fg(Color::Green),
        )));
    }

    let panel = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    frame.render_widget(panel, area);
}

fn draw_activity_feed(frame: &mut Frame<'_>, state: &SwarmDashboardState, area: Rect) {
    let focus_role = state
        .focused
        .and_then(|index| state.agents.get(index))
        .map(|agent| short_role(&agent.role));
    let mut selected: Vec<&String> = state
        .recent_activity
        .iter()
        .rev()
        .filter(|line| {
            focus_role
                .map(|role| line.starts_with(role))
                .unwrap_or(true)
        })
        .take(ACTIVITY_PANEL_LINES)
        .collect();
    selected.reverse();
    let lines: Vec<Line<'_>> = selected
        .into_iter()
        .map(|line| {
            Line::from(vec![
                Span::styled("▸ ", Style::default().fg(Color::Red)),
                Span::styled(line.clone(), Style::default().fg(Color::Gray)),
            ])
        })
        .collect();

    let title = match focus_role {
        Some(role) => format!(" Activity — {role} "),
        None => " Activity ".to_owned(),
    };
    let feed = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(feed, area);
}

fn draw_footer(frame: &mut Frame<'_>, state: &SwarmDashboardState, area: Rect) {
    let hint = if state.all_agents_terminal() {
        "all agents finished — press any key to close".to_owned()
    } else if state.has_pending_confirmation() {
        "[y] allow · [n] deny · [q] stop swarm".to_owned()
    } else {
        "[1-4] focus agent · [q] stop swarm".to_owned()
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(format!(" {hint}"), Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("   tokens {}", format_tokens(state.total_tokens)),
            Style::default().fg(Color::Gray),
        ),
    ]));
    frame.render_widget(footer, area);
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
        format!("{:.1}M", total as f64 / 1_000_000.0)
    } else if total >= 1_000 {
        format!("{:.1}K", total as f64 / 1_000.0)
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
        SwarmDashboardState::new("Build a SaaS app", "DeepSeek V4 Pro", &roles, 100)
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
        assert_eq!(format_tokens(state.total_tokens), "2.2K");
    }
}
