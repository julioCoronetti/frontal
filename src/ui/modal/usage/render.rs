use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::{UsageModal, UsageState, UsageSummary};
use crate::ui::theme::{ACCENT_HIGHLIGHT, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

fn section_header(title: &str, width: usize) -> Line<'static> {
    let prefix = format!("── {} ", title);
    let bar_len = width.saturating_sub(prefix.chars().count()).min(45);
    Line::from(vec![
        Span::styled(
            prefix,
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("─".repeat(bar_len), Style::default().fg(BORDER_SUBTLE)),
    ])
}

pub fn render_modal(modal: &UsageModal, frame: &mut Frame, area: Rect) {
    modal.poll_rx();

    if area.width < 10 || area.height < 6 {
        frame.render_widget(Clear, area);
        return;
    }

    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .padding(ratatui::widgets::Padding::new(2, 2, 1, 1))
        .title(Span::styled(
            " Antigravity CLI 1.5.3 · Quota ",
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Left)
        .style(Style::default().bg(Color::Reset));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let current_state = modal.state.lock().unwrap().clone();
    let lines = match current_state {
        UsageState::Loading => vec![Line::from(vec![Span::styled(
            "  Loading subscription and quota...",
            Style::default().fg(TEXT_MUTED),
        )])],
        UsageState::Unavailable(err) => vec![
            section_header("PLAN", inner.width as usize),
            Line::from(vec![
                Span::raw("  Your Plan: "),
                Span::styled(
                    "Antigravity Subscriber (Offline)",
                    Style::default()
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
            section_header("STATUS", inner.width as usize),
            Line::from(vec![Span::styled(
                format!("  {}", err),
                Style::default().fg(Color::Yellow),
            )]),
            Line::raw(""),
            Line::from(vec![Span::styled(
                "  Press 'r' to retry.",
                Style::default().fg(TEXT_MUTED),
            )]),
        ],
        UsageState::Ready(ref summary) => build_summary_lines(summary, inner.width as usize),
    };

    let total_lines = lines.len();
    let viewport_h = inner.height as usize;
    let limit = total_lines.saturating_sub(viewport_h);
    modal.max_scroll.set(limit);

    let active_scroll = modal.scroll_offset.min(limit);
    let paragraph = Paragraph::new(lines).scroll((active_scroll as u16, 0));
    frame.render_widget(paragraph, inner);
}

pub fn build_summary_lines(summary: &UsageSummary, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(section_header("PLAN", width));
    if let Some(ref email) = summary.plan.account_email {
        lines.push(Line::from(vec![
            Span::raw("  Account: "),
            Span::styled(
                format!("{} ({})", email, summary.plan.tier_name),
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  Your Plan: "),
            Span::styled(
                summary.plan.tier_name.clone(),
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if let Some(ref m_url) = summary.plan.manage_url {
        lines.push(Line::from(vec![Span::styled(
            format!("  Manage at: {}", m_url),
            Style::default().fg(TEXT_MUTED),
        )]));
    }
    lines.push(Line::raw(""));

    lines.push(section_header("MODEL QUOTAS", width));

    for grp in &summary.groups {
        lines.push(Line::from(vec![Span::styled(
            grp.name.to_uppercase(),
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )]));
        if let Some(ref d) = grp.description {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", d),
                Style::default().fg(TEXT_MUTED),
            )]));
        }
        lines.push(Line::raw(""));

        for b in &grp.buckets {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(b.name.clone(), Style::default().fg(TEXT_PRIMARY)),
            ]));

            let mut bar_spans = vec![Span::raw("  ")];
            bar_spans.extend(render_progress_bar_spans(b.remaining_pct, 28, b.exhausted));
            lines.push(Line::from(bar_spans));

            let reset_color = if b.exhausted {
                Color::Red
            } else if b.remaining_pct <= 20.0 {
                Color::Yellow
            } else {
                Color::Green
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(b.reset_label.clone(), Style::default().fg(reset_color)),
            ]));
            lines.push(Line::raw(""));
        }
    }

    lines
}

pub fn render_progress_bar(pct: f64, width: usize, exhausted: bool) -> (String, Color) {
    if exhausted || pct <= 0.0 {
        return ("░".repeat(width), Color::Red);
    }

    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

    let color = if clamped <= 20.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    (bar, color)
}

pub fn render_progress_bar_spans(pct: f64, width: usize, exhausted: bool) -> Vec<Span<'static>> {
    if exhausted || pct <= 0.0 {
        return vec![
            Span::raw("["),
            Span::styled("░".repeat(width), Style::default().fg(Color::DarkGray)),
            Span::raw("] "),
            Span::styled("0.00%", Style::default().fg(Color::Red)),
        ];
    }

    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let color = if clamped <= 20.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    vec![
        Span::raw("["),
        Span::styled("█".repeat(filled), Style::default().fg(color)),
        Span::styled("░".repeat(empty), Style::default().fg(Color::DarkGray)),
        Span::raw("] "),
        Span::styled(
            format!("{:.2}%", clamped),
            Style::default().fg(TEXT_PRIMARY),
        ),
    ]
}
