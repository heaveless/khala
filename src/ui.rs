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
use khala_core::metrics::Snapshot;

pub struct AppState {
    pub config: Arc<Config>,
    pub forward: Arc<khala_core::metrics::PipelineMetrics>,
    pub reverse: Arc<khala_core::metrics::PipelineMetrics>,
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
                if let Some(Ok(Event::Key(key))) = event
                    && key.kind == KeyEventKind::Press
                    && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                {
                    let _ = shutdown.send(true);
                    break;
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
    if rev.text_only {
        render_text_column(frame, right, &state.reverse_label, rev, &state.config);
    } else {
        render_column(frame, right, &state.reverse_label, rev, &state.config);
    }
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

fn render_text_column(frame: &mut Frame, area: Rect, label: &str, snap: &Snapshot, cfg: &Config) {
    let block = Block::bordered()
        .title(format!(" {label} "))
        .border_type(BorderType::Rounded);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info, input_area, transcript_area] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Min(3),
    ])
    .areas(inner);

    render_info(frame, info, snap, cfg);
    render_level(frame, input_area, "Input", snap.input_rms, &snap.input_history);
    render_transcript(frame, transcript_area, snap);
}

fn render_transcript(frame: &mut Frame, area: Rect, snap: &Snapshot) {
    let block = Block::new().borders(Borders::TOP).title(" Subtitle ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    let total_lines = inner.height as usize;
    if width == 0 || total_lines == 0 {
        return;
    }

    // Collect last 2 non-empty entries: old (top) + new (bottom).
    let non_empty: Vec<&String> = snap
        .transcript
        .iter()
        .rev()
        .filter(|e| !e.is_empty())
        .take(2)
        .collect();

    if non_empty.is_empty() {
        return;
    }

    let new_color = if snap.is_draft {
        Color::Yellow
    } else {
        Color::White
    };

    if non_empty.len() == 1 {
        // Only one subtitle — show it at the top.
        let wrapped = wrap_text(non_empty[0], width);
        let lines: Vec<Line> = wrapped
            .iter()
            .take(total_lines)
            .map(|line| Line::from(format!(" {line}")).fg(new_color))
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
    } else {
        // Two subtitles: old on top (dim), new on bottom.
        let old_text = non_empty[1]; // rev order: [0]=newest, [1]=older
        let new_text = non_empty[0];

        let old_wrapped = wrap_text(old_text, width);
        let new_wrapped = wrap_text(new_text, width);

        let old_lines = old_wrapped.len().min(total_lines / 2).max(1);
        let new_lines = (total_lines - old_lines).max(1);

        let [old_area, new_area] = Layout::vertical([
            Constraint::Length(old_lines as u16),
            Constraint::Min(new_lines as u16),
        ])
        .areas(inner);

        let old: Vec<Line> = old_wrapped
            .iter()
            .take(old_lines)
            .map(|line| Line::from(format!(" {line}")).fg(Color::DarkGray))
            .collect();
        frame.render_widget(Paragraph::new(old), old_area);

        let new: Vec<Line> = new_wrapped
            .iter()
            .take(new_lines)
            .map(|line| Line::from(format!(" {line}")).fg(new_color))
            .collect();
        frame.render_widget(Paragraph::new(new), new_area);
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let usable = width.saturating_sub(1);
    if usable == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.len() + 1 + word.len() <= usable {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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

    if inner.height < 2 {
        return;
    }

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

    if !history.is_empty() {
        let sparkline = Sparkline::default()
            .data(history)
            .max(100)
            .style(Style::default().fg(Color::Green));
        frame.render_widget(sparkline, spark_area);
    }
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
