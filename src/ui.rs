use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use futures_util::StreamExt;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::metrics::{PipelineMetrics, Snapshot};

pub struct AppState {
    pub config: Arc<Config>,
    pub forward: Arc<PipelineMetrics>,
    pub reverse: Arc<PipelineMetrics>,
    pub forward_label: String,
    pub reverse_label: String,
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

pub async fn run(
    state: AppState,
    shutdown: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let _guard = TerminalGuard;

    terminal::enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));

    loop {
        let fwd = state.forward.snapshot();
        let rev = state.reverse.snapshot();

        terminal.draw(|f| render(f, &state, &fwd, &rev))?;

        tokio::select! {
            _ = tick.tick() => {}
            event = events.next() => {
                if let Some(Ok(Event::Key(key))) = event {
                    if key.kind == KeyEventKind::Press
                        && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    {
                        let _ = shutdown.send(true);
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn render(frame: &mut Frame, state: &AppState, fwd: &Snapshot, rev: &Snapshot) {
    let [main_area, bottom] = Layout::vertical([
        Constraint::Min(16),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    let outer = Block::bordered()
        .title(" Khala Translator ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded);
    let inner = outer.inner(main_area);
    frame.render_widget(outer, main_area);

    let [left, right] = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Percentage(50),
    ])
    .spacing(1)
    .areas(inner);

    render_column(frame, left, &state.forward_label, fwd, &state.config);
    render_column(frame, right, &state.reverse_label, rev, &state.config);
    render_bar(frame, bottom, fwd, rev);
}

fn render_column(frame: &mut Frame, area: Rect, label: &str, snap: &Snapshot, cfg: &Config) {
    let block = Block::bordered()
        .title(format!(" {label} "))
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info, input_area, output_area, log_area] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Min(3),
    ])
    .areas(inner);

    render_info(frame, info, snap, cfg);
    render_level(frame, input_area, "Input", snap.input_rms, &snap.input_history);
    render_level(frame, output_area, "Output", snap.output_rms, &snap.output_history);
    render_log(frame, log_area, snap);
}

fn render_info(frame: &mut Frame, area: Rect, snap: &Snapshot, cfg: &Config) {
    let text = vec![
        Line::from(format!("  Model: {}", cfg.model)),
        Line::from(format!("  Voice: {}  │  Buf: {} samples", cfg.voice, snap.buffer_depth)),
        Line::from(format!("  Status: {}", snap.status)),
    ];
    frame.render_widget(Paragraph::new(text), area);
}

fn render_level(frame: &mut Frame, area: Rect, title: &str, rms: f32, history: &[u64]) {
    let block = Block::new().borders(Borders::TOP).title(format!(" {title} "));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [gauge_area, spark_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let pct = (rms * 100.0).min(100.0) as u16;
    let color = if rms > 0.7 {
        Color::Red
    } else if rms > 0.3 {
        Color::Yellow
    } else {
        Color::Green
    };

    let gauge = Gauge::default()
        .ratio(rms.min(1.0) as f64)
        .gauge_style(Style::default().fg(color))
        .label(format!("{pct}%"));
    frame.render_widget(gauge, gauge_area);

    let sparkline = Sparkline::default()
        .data(history)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(sparkline, spark_area);
}

fn render_log(frame: &mut Frame, area: Rect, snap: &Snapshot) {
    let block = Block::new().borders(Borders::TOP).title(" Log ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = snap
        .log
        .iter()
        .rev()
        .take(inner.height as usize)
        .map(|s| Line::from(format!(" > {s}")).fg(Color::DarkGray))
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_bar(frame: &mut Frame, area: Rect, fwd: &Snapshot, rev: &Snapshot) {
    let sent = fwd.bytes_sent + rev.bytes_sent;
    let recv = fwd.bytes_received + rev.bytes_received;
    let frames_s = fwd.frames_sent + rev.frames_sent;
    let frames_r = fwd.frames_received + rev.frames_received;

    let text = format!(
        "  Sent: {} frames ({})  │  Recv: {} frames ({})  │  'q' to quit",
        frames_s,
        fmt_bytes(sent),
        frames_r,
        fmt_bytes(recv),
    );

    let bar = Paragraph::new(text)
        .block(Block::bordered().border_type(BorderType::Rounded))
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(bar, area);
}

fn fmt_bytes(b: u64) -> String {
    if b < 1024 {
        format!("{b} B")
    } else if b < 1024 * 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    }
}
