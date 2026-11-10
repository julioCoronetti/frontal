use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::{UsageModal, UsageState, UsageSummary};
use crate::ui::theme::{ACCENT_HIGHLIGHT, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

pub fn render_modal(modal: &UsageModal, frame: &mut Frame, area: Rect) {
    modal.poll_rx();

    if area.width < 4 || area.height < 4 {
        frame.render_widget(Clear, area);
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_SUBTLE))
        .style(Style::default().bg(Color::Reset));

    let mut lines = Vec::new();
    let title = " Model Quotas:";
    let esc_pad = (area.width as usize).saturating_sub(title.chars().count() + 5);
    lines.push(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:>esc_pad$}", "esc "),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::raw(""));

    let info_title = "   Account & Subscription:";
    let nav_hint = "↑/↓: Scroll  r: Refresh ";
    let nav_pad = (area.width as usize)
        .saturating_sub(info_title.chars().count() + nav_hint.chars().count() + 5);
    lines.push(Line::from(vec![
        Span::styled(info_title, Style::default().fg(TEXT_PRIMARY)),
        Span::styled(
            format!("{:>nav_pad$}{}", "", nav_hint),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));

    let line_w = (area.width as usize).saturating_sub(8);
    let divider = "─".repeat(line_w);
    lines.push(Line::from(vec![Span::styled(
        format!("   {}", divider),
        Style::default().fg(BORDER_SUBTLE),
    )]));
    lines.push(Line::raw(""));

    let inner_height = (area.height as usize).saturating_sub(7);

    let current_state = modal.state.lock().unwrap().clone();
    let body_lines = match current_state {
        UsageState::Loading => vec![Line::from(vec![Span::styled(
            "   Loading subscription and quota...",
            Style::default().fg(TEXT_MUTED),
        )])],
        UsageState::Unavailable(err) => vec![
            Line::from(vec![
                Span::raw("   Your Plan: "),
                Span::styled(
                    "Antigravity Subscriber (Offline)",
                    Style::default()
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
            Line::from(vec![Span::styled(
                format!("   Status: {}", err),
                Style::default().fg(Color::Yellow),
            )]),
            Line::raw(""),
            Line::from(vec![Span::styled(
                "   Press 'r' to retry.",
                Style::default().fg(TEXT_MUTED),
            )]),
        ],
        UsageState::Ready(ref summary) => build_summary_lines(summary),
    };

    let total_body = body_lines.len();
    let limit = total_body.saturating_sub(inner_height);
    modal.max_scroll.set(limit);

    let active_scroll = modal.scroll_offset.min(limit);
    for line in body_lines
        .into_iter()
        .skip(active_scroll)
        .take(inner_height)
    {
        lines.push(line);
    }

    let widget = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

pub fn build_summary_lines(summary: &UsageSummary) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(ref email) = summary.plan.account_email {
        lines.push(Line::from(vec![
            Span::raw("   Account: "),
            Span::styled(
                format!("{} ({})", email, summary.plan.tier_name),
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("   Your Plan: "),
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
            format!("   Manage at: {}", m_url),
            Style::default().fg(TEXT_MUTED),
        )]));
    }
    lines.push(Line::raw(""));

    for grp in &summary.groups {
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", grp.name.to_uppercase()),
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )]));
        if let Some(ref d) = grp.description {
            let desc = if d.contains("Argon quota is separate") {
                "For paid Pro plans, Argon quota is separate."
            } else {
                d.as_str()
            };
            lines.push(Line::from(vec![Span::styled(
                format!("     {}", desc),
                Style::default().fg(TEXT_MUTED),
            )]));
        }
        lines.push(Line::raw(""));

        for b in &grp.buckets {
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(b.name.clone(), Style::default().fg(TEXT_PRIMARY)),
            ]));

            let mut bar_spans = vec![Span::raw("     ")];
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
                Span::raw("     "),
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
