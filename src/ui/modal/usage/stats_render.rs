use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::{ACCENT_HIGHLIGHT, TEXT_MUTED, TEXT_PRIMARY};

use super::stats::{OpenCodeStats, StatsState};

pub fn format_compact_count(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn build_stats_lines(state: &StatsState) -> Vec<Line<'static>> {
    match state {
        StatsState::Idle | StatsState::Loading => vec![Line::from(vec![Span::styled(
            "   Loading global OpenCode statistics...",
            Style::default().fg(TEXT_MUTED),
        )])],
        StatsState::Unavailable(err) => vec![
            Line::from(vec![Span::styled(
                "   Global statistics unavailable",
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::raw(""),
            Line::from(vec![Span::styled(
                format!("   Status: {}", err),
                Style::default().fg(Color::Yellow),
            )]),
            Line::raw(""),
            Line::from(vec![Span::styled(
                "   Press 'r' to retry or 'esc' to return.",
                Style::default().fg(TEXT_MUTED),
            )]),
        ],
        StatsState::Ready(stats) => build_ready_lines(stats),
    }
}

fn build_ready_lines(stats: &OpenCodeStats) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(section_header("OVERVIEW"));
    lines.push(kv_line(
        "Sessions / Prompts / Steps",
        format!("{} / {} / {}", stats.sessions, stats.prompts, stats.steps),
    ));
    lines.push(kv_line(
        "Subagents / Active Days",
        format!(
            "{} / {}d (streak {}d)",
            stats.subagents, stats.active_days, stats.streak
        ),
    ));
    lines.push(kv_line("Accumulated Cost", format!("${:.4}", stats.cost)));
    lines.push(Line::raw(""));

    lines.push(section_header("GLOBAL TOKENS"));
    lines.push(kv_line(
        "Input / Output",
        format!(
            "{} / {}",
            format_compact_count(stats.input_tokens),
            format_compact_count(stats.output_tokens)
        ),
    ));
    lines.push(kv_line(
        "Reasoning / Cache Read",
        format!(
            "{} / {}",
            format_compact_count(stats.reasoning_tokens),
            format_compact_count(stats.cache_read_tokens)
        ),
    ));
    if stats.cache_write_tokens > 0 {
        lines.push(kv_line(
            "Cache Write",
            format_compact_count(stats.cache_write_tokens),
        ));
    }
    lines.push(Line::raw(""));

    lines.push(section_header("CONSUMPTION BY MODEL"));
    if stats.models.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "     No model usage recorded.",
            Style::default().fg(TEXT_MUTED),
        )]));
    } else {
        for m in &stats.models {
            let full_id = if m.provider_id.is_empty() {
                m.id.clone()
            } else {
                format!("{}/{}", m.provider_id, m.id)
            };
            let display = crate::ui::labels::model_display_name(&full_id);
            let label = match m.variant.as_deref() {
                Some(v) => format!("{} ({})", display, v),
                None => display,
            };
            let total_tok = m
                .input_tokens
                .saturating_add(m.output_tokens)
                .saturating_add(m.reasoning_tokens)
                .saturating_add(m.cache_read_tokens);
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(label, Style::default().fg(TEXT_PRIMARY)),
            ]));
            lines.push(Line::from(vec![
                Span::raw("       "),
                Span::styled(
                    format!(
                        "{} steps · {} tokens · ${:.4}",
                        m.steps,
                        format_compact_count(total_tok),
                        m.cost
                    ),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]));
        }
    }
    lines.push(Line::raw(""));

    lines.push(section_header("TOOL RELIABILITY"));
    if stats.tool_calls == 0 && stats.tools.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "     No tool calls recorded.",
            Style::default().fg(TEXT_MUTED),
        )]));
    } else {
        let rate = if stats.tool_calls > 0 {
            ((stats.tool_succeeded as f64 / stats.tool_calls as f64) * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        };
        let rate_color = reliability_color(rate);
        lines.push(Line::from(vec![
            Span::raw("     Overall: "),
            Span::styled(
                format!(
                    "{:.1}% ({}/{} ok, {} failed)",
                    rate, stats.tool_succeeded, stats.tool_calls, stats.tool_failed
                ),
                Style::default().fg(rate_color).add_modifier(Modifier::BOLD),
            ),
        ]));
        for t in &stats.tools {
            let t_rate = if t.calls > 0 {
                ((t.succeeded as f64 / t.calls as f64) * 100.0).clamp(0.0, 100.0)
            } else {
                100.0
            };
            let tool_label: String = t.name.chars().take(12).collect();
            let p50 = t
                .duration_p50_ms
                .map(|ms| format!(" · p50 {:.0}ms", ms))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(
                    format!("{:<12} ", tool_label),
                    Style::default().fg(TEXT_PRIMARY),
                ),
                Span::styled(
                    format!("{:.1}%", t_rate),
                    Style::default().fg(reliability_color(t_rate)),
                ),
                Span::styled(
                    format!(" ({}/{}, {} err{})", t.succeeded, t.calls, t.failed, p50),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]));
        }
    }
    lines.push(Line::raw(""));

    lines
}

fn section_header(title: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        format!("   {}", title),
        Style::default()
            .fg(TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )])
}

fn kv_line(label: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("     {}: ", label), Style::default().fg(TEXT_MUTED)),
        Span::styled(
            value,
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn reliability_color(rate: f64) -> Color {
    if rate >= 95.0 {
        Color::Green
    } else if rate >= 80.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}
