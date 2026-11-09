use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::{UsageModal, UsageState, UsageSummary};
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

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
            " Antigravity CLI 1.5.3 · Usage & Quota ",
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
        UsageState::Loading => vec![
            Line::raw(""),
            Line::from(vec![Span::styled(
                "  Loading subscription and quota...",
                Style::default().fg(TEXT_MUTED),
            )]),
        ],
        UsageState::Unavailable(err) => vec![
            Line::raw(""),
            Line::from(vec![Span::styled(
                "PLAN",
                Style::default()
                    .fg(ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
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
            Line::from(vec![Span::styled(
                "STATUS",
                Style::default()
                    .fg(ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )]),
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
        UsageState::Ready(ref summary) => build_summary_lines(summary),
    };

    let total_lines = lines.len();
    let viewport_h = inner.height as usize;
    let limit = total_lines.saturating_sub(viewport_h);
    modal.max_scroll.set(limit);

    let active_scroll = modal.scroll_offset.min(limit);
    let paragraph = Paragraph::new(lines).scroll((active_scroll as u16, 0));
    frame.render_widget(paragraph, inner);
}

pub fn build_summary_lines(summary: &UsageSummary) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::raw(""));

    lines.push(Line::from(vec![Span::styled(
        "PLAN",
        Style::default()
            .fg(ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )]));
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

    lines.push(Line::from(vec![Span::styled(
        "MODEL QUOTAS",
        Style::default()
            .fg(ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )]));

    for grp in &summary.groups {
        lines.push(Line::from(vec![Span::styled(
            format!("  {}", grp.name),
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )]));
        if let Some(ref d) = grp.description {
            lines.push(Line::from(vec![Span::styled(
                format!("    {}", d),
                Style::default().fg(TEXT_MUTED),
            )]));
        }

        for b in &grp.buckets {
            let (bar, bar_color) = render_progress_bar(b.remaining_pct, 14, b.exhausted);
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(b.name.clone(), Style::default().fg(TEXT_PRIMARY)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("    ["),
                Span::styled(bar, Style::default().fg(bar_color)),
                Span::raw("] "),
                Span::styled(
                    format!("{:.1}%", b.remaining_pct),
                    Style::default().fg(TEXT_PRIMARY),
                ),
                Span::raw("  "),
                Span::styled(b.reset_label.clone(), Style::default().fg(TEXT_MUTED)),
            ]));
            if b.exhausted {
                lines.push(Line::from(vec![Span::styled(
                    "    Disabled: Quota exhausted",
                    Style::default().fg(Color::Red),
                )]));
            }
        }
        lines.push(Line::raw(""));
    }

    lines.push(Line::from(vec![Span::styled(
        "MODEL CREDITS",
        Style::default()
            .fg(ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )]));

    if let Some(ref cred) = summary.credits {
        let avail = cred.available.as_deref().unwrap_or("$0.00");
        let over = cred.overages.as_deref().unwrap_or("$0.00");
        lines.push(Line::from(vec![
            Span::raw("  Available AI Credits: "),
            Span::styled(avail.to_string(), Style::default().fg(TEXT_PRIMARY)),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  AI Credit Overages:   "),
            Span::styled(over.to_string(), Style::default().fg(TEXT_PRIMARY)),
        ]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "  No AI credits available for this account",
            Style::default().fg(TEXT_MUTED),
        )]));
    }
    lines.push(Line::raw(""));

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
