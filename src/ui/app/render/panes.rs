use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BG_SIDEBAR, TEXT_MUTED, TEXT_PRIMARY};

impl App {
    pub(crate) fn render_chat(
        &mut self,
        frame: &mut Frame,
        chat_area: Rect,
        is_welcome: bool,
        spinner_char: &str,
    ) {
        if is_welcome {
            let bold_hl = Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD);
            let welcome_lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "frontal",
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" v{}", env!("CARGO_PKG_VERSION")),
                        Style::default().fg(TEXT_MUTED),
                    ),
                ]),
                Line::from(vec![Span::styled(
                    "Async TUI for Antigravity & stream-json engines",
                    Style::default().fg(TEXT_MUTED),
                )]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Type a prompt below or use slash commands:",
                    Style::default().fg(TEXT_PRIMARY),
                )]),
                Line::from(vec![
                    Span::styled("/help", bold_hl),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("/model", bold_hl),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("/diff", bold_hl),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled("Ctrl+P", bold_hl),
                ]),
            ];
            let welcome_widget =
                Paragraph::new(welcome_lines).alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(welcome_widget, chat_area);
        } else {
            let total_lines = self.cache.ensure_visual_lines(chat_area.width);
            let queued_count = self.queued_prompts.len();
            let total_with_spinner = total_lines
                + if self.waiting_for_response { 1 } else { 0 }
                + if queued_count > 0 { 1 } else { 0 };
            self.viewport
                .sync_dimensions(total_with_spinner, chat_area.height as usize);
            let mut visible_lines = self.viewport.visible_slice(&self.cache);

            if self.waiting_for_response {
                visible_lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", spinner_char),
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Thinking / Waiting for response...",
                        Style::default()
                            .fg(TEXT_MUTED)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }

            if queued_count > 0 {
                visible_lines.push(Line::from(vec![
                    Span::styled("⏳ ", Style::default().fg(ACCENT_PRIMARY)),
                    Span::styled(
                        format!("Queued ({} pending)...", queued_count),
                        Style::default()
                            .fg(TEXT_MUTED)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }

            frame.render_widget(Clear, chat_area);
            let viewport_widget = Paragraph::new(visible_lines);
            frame.render_widget(viewport_widget, chat_area);

            if !self.viewport.is_anchored()
                && self.viewport.total_lines() > self.viewport.viewport_height()
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
        let inner = Rect {
            x: sb_area.x.saturating_add(1),
            y: sb_area.y.saturating_add(1),
            width: sb_area.width.saturating_sub(2),
            height: sb_area.height.saturating_sub(1),
        };
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let used_tokens = self
            .last_turn_usage
            .as_ref()
            .map(|usage| usage.effective_total())
            .unwrap_or(0);
        let spec = self.active_model_spec();
        let mut sb_lines = vec![Line::from(vec![Span::styled(
            "Context",
            Style::default()
                .fg(TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )])];
        sb_lines.push(Line::from(vec![Span::styled(
            format!("{} tokens", format_token_count(used_tokens)),
            Style::default().fg(TEXT_MUTED),
        )]));
        match spec.occupancy_pct(used_tokens) {
            Some(pct) => sb_lines.push(Line::from(vec![Span::styled(
                format!("{:.0}% used", pct),
                Style::default().fg(TEXT_MUTED),
            )])),
            None => sb_lines.push(Line::from(vec![Span::styled(
                "window unknown",
                Style::default().fg(TEXT_MUTED),
            )])),
        }
        frame.render_widget(Paragraph::new(sb_lines), inner);
    }
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
