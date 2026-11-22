use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::{build_stats_lines, StatsState, UsageModal, UsagePage, UsageState, UsageSummary};
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum CachedBodyKey {
    Main {
        wrap_width: usize,
        is_opencode: bool,
        state: UsageState,
    },
    Stats {
        wrap_width: usize,
        state: StatsState,
    },
}

#[derive(Clone, Debug, Default)]
pub(super) struct UsageBodyCache {
    key: Option<CachedBodyKey>,
    lines: Vec<Line<'static>>,
}

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
    let (title, esc_label) = match modal.page {
        UsagePage::Main => (" Model Quotas:", "esc "),
        UsagePage::OpenCodeStats => (" OpenCode Global Stats:", "esc: Back "),
    };
    let esc_pad = (area.width as usize)
        .saturating_sub(title.chars().count() + 5)
        .max(esc_label.chars().count() + 1);
    lines.push(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:>esc_pad$}", esc_label),
            Style::default().fg(TEXT_MUTED),
        ),
    ]));
    lines.push(Line::raw(""));

    let (info_title, nav_hint) = match (modal.is_opencode, modal.page) {
        (true, UsagePage::Main) => ("   Account & Subscription:", "Enter: Stats"),
        (_, UsagePage::OpenCodeStats) => ("   Global Usage & Stats:", "r: Refresh"),
        (false, UsagePage::Main) => ("   Account & Subscription:", "r: Refresh"),
    };
    let nav_pad = (area.width as usize)
        .saturating_sub(info_title.chars().count() + nav_hint.chars().count() + 5)
        .max(1);
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
    let wrap_width = (area.width as usize).saturating_sub(5).max(1);

    let mut cache = modal.body_cache.borrow_mut();
    match modal.page {
        UsagePage::OpenCodeStats => {
            let stats_state = modal.stats_state.lock().unwrap_or_else(|e| e.into_inner());
            let hit = matches!(
                cache.key.as_ref(),
                Some(CachedBodyKey::Stats {
                    wrap_width: w,
                    state,
                }) if *w == wrap_width && *state == *stats_state
            );
            if !hit {
                let raw_lines = build_stats_lines(&stats_state);
                cache.key = Some(CachedBodyKey::Stats {
                    wrap_width,
                    state: stats_state.clone(),
                });
                drop(stats_state);
                cache.lines =
                    crate::ui::markdown::wrap_lines_preserving_indent(raw_lines, wrap_width);
            }
        }
        UsagePage::Main => {
            let current_state = modal.state.lock().unwrap_or_else(|e| e.into_inner());
            let hit = matches!(
                cache.key.as_ref(),
                Some(CachedBodyKey::Main {
                    wrap_width: w,
                    is_opencode,
                    state,
                }) if *w == wrap_width
                    && *is_opencode == modal.is_opencode
                    && *state == *current_state
            );
            if !hit {
                let mut body = match &*current_state {
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
                    UsageState::Ready(summary) => build_summary_lines(summary),
                };
                if modal.is_opencode {
                    body.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            " ● Global Usage & Tool Stats ",
                            Style::default()
                                .bg(ACCENT_RUBY)
                                .fg(TEXT_PRIMARY)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            "[Enter] ",
                            Style::default()
                                .bg(ACCENT_RUBY)
                                .fg(ACCENT_HIGHLIGHT)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    body.push(Line::raw(""));
                }
                cache.key = Some(CachedBodyKey::Main {
                    wrap_width,
                    is_opencode: modal.is_opencode,
                    state: current_state.clone(),
                });
                drop(current_state);
                cache.lines = crate::ui::markdown::wrap_lines_preserving_indent(body, wrap_width);
            }
        }
    }

    let total_body = cache.lines.len();
    let limit = total_body.saturating_sub(inner_height);
    modal.max_scroll.set(limit);

    let active_scroll = modal.scroll_offset.min(limit);
    lines.extend(
        cache
            .lines
            .iter()
            .skip(active_scroll)
            .take(inner_height)
            .cloned(),
    );
    drop(cache);

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
