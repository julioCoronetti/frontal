use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

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
        let used_tokens = self
            .last_turn_usage
            .as_ref()
            .map(|usage| usage.effective_total())
            .unwrap_or(0);
        let spec = self.active_model_spec();
        let window = spec.usable();
        let occupancy = spec.occupancy_pct(used_tokens);
        let mut sb_lines = vec![
            Line::from(vec![
                Span::styled("● ", Style::default().fg(Color::Green)),
                Span::styled(
                    "Session Active",
                    Style::default()
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("ID: ", Style::default().fg(TEXT_MUTED)),
                Span::styled(
                    self.active_session.as_str(),
                    Style::default().fg(TEXT_PRIMARY),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Context Window",
                Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
            )]),
        ];
        match (window, occupancy) {
            (Some(tokens), Some(pct)) => {
                sb_lines.push(Line::from(vec![Span::styled(
                    format!("{} tok ({:.1}%)", used_tokens, pct),
                    Style::default().fg(TEXT_PRIMARY),
                )]));
                let filled = ((pct / 100.0) * 14.0).round().clamp(0.0, 14.0) as usize;
                let bar = format!(
                    "{}{}",
                    "■".repeat(filled),
                    "░".repeat(14usize.saturating_sub(filled))
                );
                sb_lines.push(Line::from(vec![Span::styled(
                    bar,
                    Style::default().fg(ACCENT_PRIMARY),
                )]));
                let remaining = spec.remaining(used_tokens).unwrap_or(tokens);
                sb_lines.push(Line::from(vec![Span::styled(
                    format!("Remaining: {} tok", remaining),
                    Style::default().fg(TEXT_MUTED),
                )]));
                if spec.is_compaction_recommended(used_tokens) {
                    sb_lines.push(Line::from(vec![Span::styled(
                        "Compact now (>=75%)",
                        Style::default()
                            .fg(ACCENT_HIGHLIGHT)
                            .add_modifier(Modifier::BOLD),
                    )]));
                } else if let Some(margin) = spec.compact_margin(used_tokens) {
                    sb_lines.push(Line::from(vec![Span::styled(
                        format!("Compact in {} tok", margin),
                        Style::default().fg(TEXT_MUTED),
                    )]));
                }
            }
            _ => {
                sb_lines.push(Line::from(vec![Span::styled(
                    format!("{} tok · window open", used_tokens),
                    Style::default().fg(TEXT_PRIMARY),
                )]));
                sb_lines.push(Line::from(vec![Span::styled(
                    "limit unknown",
                    Style::default().fg(TEXT_MUTED),
                )]));
            }
        }
        sb_lines.extend(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                format!(
                    "Steps: {} · Turns: {}",
                    self.steps_seen, self.turns_completed
                ),
                Style::default().fg(TEXT_PRIMARY),
            )]),
        ]);
        let cache_read = self
            .last_turn_usage
            .map(|usage| usage.cache_read_tokens)
            .unwrap_or(0);
        if cache_read > 0 || self.accumulated_usage.effective_total() > 0 {
            sb_lines.push(Line::from(vec![Span::styled(
                format!(
                    "Cache: {} tok · {:.1}%",
                    cache_read,
                    self.accumulated_usage.cache_hit_percentage()
                ),
                Style::default().fg(TEXT_MUTED),
            )]));
        }
        if let Some(duration) = self.last_turn_duration_seconds {
            sb_lines.push(Line::from(vec![Span::styled(
                format!("Last turn: {:.1}s", duration),
                Style::default().fg(TEXT_MUTED),
            )]));
        }
        sb_lines.extend(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "Workspace",
                Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                &self.cwd_display,
                Style::default().fg(TEXT_MUTED),
            )]),
        ]);
        let sb_widget = Paragraph::new(sb_lines).block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(BORDER_SUBTLE))
                .title(" Context "),
        );
        frame.render_widget(sb_widget, sb_area);
    }
}
