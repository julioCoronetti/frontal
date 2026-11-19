use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::theme::{ACCENT_HIGHLIGHT, TEXT_MUTED, TEXT_PRIMARY};

pub static OPENCODE_STATS_CACHE: Mutex<Option<(Instant, OpenCodeStats)>> = Mutex::new(None);
static STATS_FETCH_STATE: Mutex<StatsFetchState> = Mutex::new(StatsFetchState {
    in_flight: false,
    waiters: Vec::new(),
});

struct StatsFetchState {
    in_flight: bool,
    waiters: Vec<Sender<StatsState>>,
}

struct StatsFetchGuard;

impl Drop for StatsFetchGuard {
    fn drop(&mut self) {
        let waiters = {
            let mut state = STATS_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
            if !state.in_flight {
                return;
            }
            state.in_flight = false;
            std::mem::take(&mut state.waiters)
        };
        let err_state = StatsState::Unavailable("Stats worker interrupted".to_string());
        for waiter in waiters {
            let _ = waiter.send(err_state.clone());
        }
    }
}

fn sanitize_label(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect()
}

fn sanitize_non_neg_f64(val: f64) -> f64 {
    if val.is_finite() && val >= 0.0 {
        val
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UsagePage {
    #[default]
    Main,
    OpenCodeStats,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCodeModelStat {
    pub id: String,
    pub provider_id: String,
    pub variant: Option<String>,
    pub steps: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenCodeToolStat {
    pub name: String,
    pub calls: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub duration_p50_ms: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenCodeStats {
    pub sessions: u64,
    pub subagents: u64,
    pub prompts: u64,
    pub steps: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost: f64,
    pub active_days: u64,
    pub streak: u64,
    pub tool_calls: u64,
    pub tool_succeeded: u64,
    pub tool_failed: u64,
    pub tool_unfinished: u64,
    pub models: Vec<OpenCodeModelStat>,
    pub tools: Vec<OpenCodeToolStat>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatsState {
    Idle,
    Loading,
    Ready(OpenCodeStats),
    Unavailable(String),
}

pub fn parse_opencode_stats_json(raw: &str) -> Result<OpenCodeStats, String> {
    let cleaned = crate::transport::strip_ansi(raw);
    let val: serde_json::Value =
        serde_json::from_str(cleaned.trim()).map_err(|e| format!("Invalid stats JSON: {}", e))?;

    let tokens = val.get("tokens");
    let cache = tokens.and_then(|t| t.get("cache"));
    let tools_obj = val.get("tools");
    let totals = tools_obj.and_then(|t| t.get("totals"));

    let mut models = Vec::new();
    if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
        for item in arr {
            let model_meta = item.get("model");
            let id = sanitize_label(
                model_meta
                    .and_then(|m| m.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            );
            let provider_id = sanitize_label(
                model_meta
                    .and_then(|m| m.get("providerID"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
            );
            let variant = model_meta
                .and_then(|m| m.get("variant"))
                .and_then(|v| v.as_str())
                .map(sanitize_label)
                .filter(|v| !v.is_empty() && v != "default");
            let m_tokens = item.get("tokens");
            let m_cache = m_tokens.and_then(|t| t.get("cache"));
            models.push(OpenCodeModelStat {
                id,
                provider_id,
                variant,
                steps: item.get("steps").and_then(|v| v.as_u64()).unwrap_or(0),
                input_tokens: m_tokens
                    .and_then(|t| t.get("input"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                output_tokens: m_tokens
                    .and_then(|t| t.get("output"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                reasoning_tokens: m_tokens
                    .and_then(|t| t.get("reasoning"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_read_tokens: m_cache
                    .and_then(|c| c.get("read"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cost: sanitize_non_neg_f64(
                    item.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0),
                ),
            });
        }
    }
    models.sort_by_key(|a| std::cmp::Reverse(a.steps));

    let mut tools = Vec::new();
    if let Some(arr) = tools_obj
        .and_then(|t| t.get("usage"))
        .and_then(|u| u.as_array())
    {
        for item in arr {
            let name = sanitize_label(
                item.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            );
            tools.push(OpenCodeToolStat {
                name,
                calls: item.get("calls").and_then(|v| v.as_u64()).unwrap_or(0),
                succeeded: item.get("succeeded").and_then(|v| v.as_u64()).unwrap_or(0),
                failed: item.get("failed").and_then(|v| v.as_u64()).unwrap_or(0),
                duration_p50_ms: item
                    .get("durationP50")
                    .and_then(|v| v.as_f64())
                    .filter(|v| v.is_finite() && *v >= 0.0),
            });
        }
    }
    tools.sort_by_key(|a| std::cmp::Reverse(a.calls));

    Ok(OpenCodeStats {
        sessions: val.get("sessions").and_then(|v| v.as_u64()).unwrap_or(0),
        subagents: val.get("subagents").and_then(|v| v.as_u64()).unwrap_or(0),
        prompts: val.get("prompts").and_then(|v| v.as_u64()).unwrap_or(0),
        steps: val.get("steps").and_then(|v| v.as_u64()).unwrap_or(0),
        input_tokens: tokens
            .and_then(|t| t.get("input"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output_tokens: tokens
            .and_then(|t| t.get("output"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        reasoning_tokens: tokens
            .and_then(|t| t.get("reasoning"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_read_tokens: cache
            .and_then(|c| c.get("read"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_write_tokens: cache
            .and_then(|c| c.get("write"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cost: sanitize_non_neg_f64(val.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0)),
        active_days: val.get("activeDays").and_then(|v| v.as_u64()).unwrap_or(0),
        streak: val.get("streak").and_then(|v| v.as_u64()).unwrap_or(0),
        tool_calls: totals
            .and_then(|t| t.get("calls"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_succeeded: totals
            .and_then(|t| t.get("succeeded"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_failed: totals
            .and_then(|t| t.get("failed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        tool_unfinished: totals
            .and_then(|t| t.get("unfinished"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        models,
        tools,
    })
}

pub fn fetch_stats_async(tx: Sender<StatsState>) {
    {
        let mut state = STATS_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
        state.waiters.push(tx);
        if state.in_flight {
            return;
        }
        state.in_flight = true;
    }
    std::thread::spawn(move || {
        let _guard = StatsFetchGuard;
        let state = match crate::transport::run_opencode_command(
            &["stats", "--json", "--models", "--tools", "--cost"],
            Duration::from_secs(5),
        ) {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes);
                match parse_opencode_stats_json(&raw) {
                    Ok(stats) => StatsState::Ready(stats),
                    Err(err) => StatsState::Unavailable(err),
                }
            }
            Err(err) => StatsState::Unavailable(err),
        };
        if let StatsState::Ready(ref stats) = state {
            let mut cache = OPENCODE_STATS_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *cache = Some((Instant::now(), stats.clone()));
        }
        let waiters: Vec<Sender<StatsState>> = {
            let mut fetch_state = STATS_FETCH_STATE.lock().unwrap_or_else(|e| e.into_inner());
            fetch_state.in_flight = false;
            std::mem::take(&mut fetch_state.waiters)
        };
        for waiter in waiters {
            let _ = waiter.send(state.clone());
        }
    });
}

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
