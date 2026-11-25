use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::core::model_spec::format_compact_tokens;
use crate::ui::app::render::semaphore_color;
use crate::ui::app::App;
use crate::ui::theme::{ACCENT_PRIMARY, BG_SIDEBAR, TEXT_INTERRUPT_HINT, TEXT_MUTED, TEXT_PRIMARY};

impl App {
    pub(crate) fn render_chat(
        &mut self,
        frame: &mut Frame,
        chat_area: Rect,
        is_welcome: bool,
        spinner_char: &str,
    ) {
        if is_welcome {
            self.render_welcome_compact(frame, chat_area);
        } else {
            let tab = self.active_tab_mut();
            let total_lines = tab.cache.ensure_visual_lines(chat_area.width);
            let queued_count = tab.queued_prompts.len();
            let armed_interrupt = tab.is_interrupt_armed();
            let show_turn_indicator = tab.is_waiting_for_response() || armed_interrupt;
            let total_with_spinner = total_lines
                + if show_turn_indicator { 1 } else { 0 }
                + if queued_count > 0 { 1 } else { 0 };
            tab.viewport
                .sync_dimensions(total_with_spinner, chat_area.height as usize);
            let mut visible_lines = tab.viewport.visible_slice(&tab.cache);

            let status_margin = if chat_area.width > 4 { "  " } else { "" };
            if show_turn_indicator {
                let mut spans = Vec::new();
                if !status_margin.is_empty() {
                    spans.push(Span::raw(status_margin));
                }
                spans.push(Span::styled(
                    format!("{} ", spinner_char),
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ));
                if armed_interrupt {
                    let width = chat_area.width as usize;
                    if width >= 39 {
                        spans.push(Span::styled(
                            "Thinking...",
                            Style::default()
                                .fg(TEXT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ));
                        spans.push(Span::raw("  "));
                        spans.push(Span::styled(
                            "esc again to interrupt",
                            Style::default()
                                .fg(TEXT_INTERRUPT_HINT)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else if width >= 26 {
                        spans.push(Span::styled(
                            "esc again to interrupt",
                            Style::default()
                                .fg(TEXT_INTERRUPT_HINT)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        spans.push(Span::styled(
                            "esc×2",
                            Style::default()
                                .fg(TEXT_INTERRUPT_HINT)
                                .add_modifier(Modifier::BOLD),
                        ));
                    }
                } else {
                    spans.push(Span::styled(
                        "Thinking...",
                        Style::default()
                            .fg(TEXT_MUTED)
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
                visible_lines.push(Line::from(spans));
            }

            if queued_count > 0 {
                let mut spans = Vec::new();
                if !status_margin.is_empty() {
                    spans.push(Span::raw(status_margin));
                }
                spans.push(Span::styled("⏳ ", Style::default().fg(ACCENT_PRIMARY)));
                spans.push(Span::styled(
                    format!("Queued ({} pending)...", queued_count),
                    Style::default()
                        .fg(TEXT_MUTED)
                        .add_modifier(Modifier::ITALIC),
                ));
                visible_lines.push(Line::from(spans));
            }

            frame.render_widget(Clear, chat_area);
            let viewport_widget = Paragraph::new(visible_lines);
            frame.render_widget(viewport_widget, chat_area);

            if !self.active_tab().viewport.is_anchored()
                && self.active_tab().viewport.total_lines()
                    > self.active_tab().viewport.viewport_height()
            {
                let indicator_w = 20u16.min(chat_area.width);
                let indicator_rect = Rect {
                    x: chat_area
                        .x
                        .saturating_add(chat_area.width.saturating_sub(indicator_w + 1)),
                    y: chat_area
                        .y
                        .saturating_add(chat_area.height.saturating_sub(1)),
                    width: indicator_w,
                    height: 1,
                };
                let indicator_widget = Paragraph::new(Line::from(vec![Span::styled(
                    " ▼ Scroll to bottom ",
                    Style::default()
                        .bg(ACCENT_PRIMARY)
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                )]));
                frame.render_widget(Clear, indicator_rect);
                frame.render_widget(indicator_widget, indicator_rect);
            }
        }
    }

    pub(crate) fn render_sidebar(&self, frame: &mut Frame, sb_area: Rect) {
        if sb_area.width == 0 || sb_area.height == 0 {
            return;
        }
        frame.render_widget(
            Block::default().style(Style::default().bg(BG_SIDEBAR)),
            sb_area,
        );
        let used_tokens = self
            .active_tab()
            .last_turn_usage
            .as_ref()
            .map(|usage| usage.effective_total())
            .unwrap_or(0);
        let spec = self.active_model_spec();
        let bar_style = Style::default()
            .fg(ACCENT_PRIMARY)
            .bg(BG_SIDEBAR)
            .add_modifier(Modifier::BOLD);
        let mut sb_lines = vec![
            Line::from(vec![Span::styled("▌", bar_style)]),
            sidebar_line("Context", TEXT_PRIMARY, true, bar_style),
            sidebar_line(
                &format!("{} tokens", format_token_count(used_tokens)),
                TEXT_MUTED,
                false,
                bar_style,
            ),
        ];
        match (spec.occupancy_pct(used_tokens), spec.semaphore(used_tokens)) {
            (Some(pct), Some(sem)) => {
                let summary = match spec.compaction_threshold() {
                    Some(threshold) => format!(
                        " {:.0}% used ({}/{})",
                        pct,
                        format_compact_tokens(used_tokens),
                        format_compact_tokens(threshold)
                    ),
                    None => format!(" {:.0}% used", pct),
                };
                sb_lines.push(Line::from(vec![
                    Span::styled("▌", bar_style),
                    Span::styled(" ", Style::default().bg(BG_SIDEBAR)),
                    Span::styled(
                        "●",
                        Style::default().fg(semaphore_color(sem)).bg(BG_SIDEBAR),
                    ),
                    Span::styled(summary, Style::default().fg(TEXT_MUTED).bg(BG_SIDEBAR)),
                ]));
            }
            _ => sb_lines.push(sidebar_line("window unknown", TEXT_MUTED, false, bar_style)),
        }
        while sb_lines.len() < sb_area.height as usize {
            sb_lines.push(Line::from(vec![Span::styled("▌", bar_style)]));
        }
        frame.render_widget(Paragraph::new(sb_lines), sb_area);
    }
}

fn sidebar_line(text: &str, color: Color, bold: bool, bar_style: Style) -> Line<'static> {
    let mut text_style = Style::default().fg(color).bg(BG_SIDEBAR);
    if bold {
        text_style = text_style.add_modifier(Modifier::BOLD);
    }
    Line::from(vec![
        Span::styled("▌", bar_style),
        Span::raw(" "),
        Span::styled(text.to_string(), text_style),
    ])
}

fn format_token_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}
