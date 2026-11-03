use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::app::App;
use crate::ui::editor::TriggerKind;
use crate::ui::theme::{ACCENT_PRIMARY, BORDER_SUBTLE, TAB_ACTIVE_DOT, TEXT_MUTED, TEXT_PRIMARY};

impl App {
    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame.render_widget(Clear, area);
        let padded_area = if area.width > 16 {
            Rect {
                x: area.x.saturating_add(2),
                y: area.y,
                width: area.width.saturating_sub(4),
                height: area.height,
            }
        } else {
            area
        };

        let input_lines = (self.editor.line_count() as u16).clamp(1, 8);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(input_lines + 2),
                Constraint::Length(1),
            ])
            .split(padded_area);

        let tab_bar_area = chunks[0];
        let _spacer_area = chunks[1];
        let viewport_area = chunks[2];
        let input_area = chunks[3];
        let footer_area = chunks[4];
        self.viewport_bounds = (
            viewport_area.x,
            viewport_area.y,
            viewport_area.width,
            viewport_area.height,
        );

        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let spinner_char = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];

        let mut tab_spans = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab_index;
            let num = i + 1;
            let unread_marker = if tab.has_unread { "*" } else { "" };
            let is_waiting = if is_active {
                self.waiting_for_response
            } else {
                tab.waiting_for_response
            };
            let spinner = if is_waiting {
                format!("{} ", spinner_char)
            } else {
                String::new()
            };
            let dot = if is_active { "● " } else { "○ " };
            let dot_color = if is_active {
                TAB_ACTIVE_DOT
            } else {
                TEXT_MUTED
            };
            let text_color = if is_active {
                TEXT_PRIMARY
            } else if tab.has_unread {
                Color::Yellow
            } else {
                TEXT_MUTED
            };
            tab_spans.push(Span::styled(dot, Style::default().fg(dot_color)));
            tab_spans.push(Span::styled(
                format!("{}: {}{}{}", num, spinner, tab.title, unread_marker),
                Style::default().fg(text_color).add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ));
            tab_spans.push(Span::raw("   "));
        }
        let tab_bar_widget = Paragraph::new(Line::from(tab_spans));
        frame.render_widget(tab_bar_widget, tab_bar_area);

        let divider_widget = Paragraph::new(Line::from(vec![Span::styled(
            "─".repeat(padded_area.width as usize),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        frame.render_widget(divider_widget, chunks[1]);

        let is_welcome = self.cache.blocks_count() == 0 && !self.waiting_for_response;
        let (chat_area, sidebar_area) = if !is_welcome && padded_area.width >= 100 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(60), Constraint::Length(28)])
                .split(viewport_area);
            (cols[0], Some(cols[1]))
        } else {
            (viewport_area, None)
        };
        self.viewport_bounds = (chat_area.x, chat_area.y, chat_area.width, chat_area.height);

        if is_welcome {
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
                    Span::styled(
                        "/help",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled(
                        "/model",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled(
                        "/diff",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" · ", Style::default().fg(TEXT_MUTED)),
                    Span::styled(
                        "/palette",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
            ];
            let welcome_widget =
                Paragraph::new(welcome_lines).alignment(ratatui::layout::Alignment::Center);
            frame.render_widget(welcome_widget, chat_area);
        } else {
            let total_lines = self.cache.ensure_visual_lines(chat_area.width);
            let total_with_spinner = total_lines + if self.waiting_for_response { 1 } else { 0 };
            self.viewport
                .sync_dimensions(total_with_spinner, chat_area.height as usize);
            let mut visible_lines = self.viewport.visible_slice(&self.cache);

            if self.waiting_for_response {
                const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let spinner =
                    SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];
                visible_lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(
                        format!("{} ", spinner),
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
                        .fg(Color::Black)
                        .add_modifier(Modifier::BOLD),
                )]));
                frame.render_widget(Clear, indicator_rect);
                frame.render_widget(indicator_widget, indicator_rect);
            }
        }

        if let Some(sb_area) = sidebar_area {
            let total_tokens = self
                .last_turn_usage
                .as_ref()
                .map(|u| u.total_tokens)
                .unwrap_or(0);
            let pct = (total_tokens as f64 / 1_000_000.0 * 100.0).min(100.0);
            let filled_blocks = ((pct / 100.0) * 14.0).round() as usize;
            let bar = format!(
                "{}{}",
                "■".repeat(filled_blocks),
                "░".repeat(14usize.saturating_sub(filled_blocks))
            );
            let sb_lines = vec![
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
                Line::from(vec![Span::styled(
                    format!("{} tok ({:.1}%)", total_tokens, pct),
                    Style::default().fg(TEXT_PRIMARY),
                )]),
                Line::from(vec![Span::styled(bar, Style::default().fg(ACCENT_PRIMARY))]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    "Workspace",
                    Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![Span::styled(
                    &self.cwd_display,
                    Style::default().fg(TEXT_MUTED),
                )]),
            ];
            let sb_widget = Paragraph::new(sb_lines).block(
                Block::default()
                    .borders(Borders::LEFT)
                    .border_style(Style::default().fg(BORDER_SUBTLE))
                    .title(" Context "),
            );
            frame.render_widget(sb_widget, sb_area);
        }

        let model_name = self.active_model_name();
        let mode_name = self.active_mode_name();
        let effort_name = self.active_effort_name();
        let input_title = if model_name.ends_with(&format!("-{}", effort_name)) {
            format!(" Input [{}] · {} ", mode_name, model_name)
        } else {
            format!(" Input [{}] · {} ({}) ", mode_name, model_name, effort_name)
        };
        let display_content = self.editor.text();
        let (cur_line, cur_col) = self.editor.cursor_coordinates();

        let max_visible = input_area.height.saturating_sub(2) as usize;
        let scroll_y = if max_visible > 0 && cur_line >= max_visible {
            (cur_line + 1 - max_visible) as u16
        } else {
            0
        };

        let input_text = if let Some((sel_start, sel_end)) = self.editor.selection_range() {
            let full_text = self.editor.text();
            let mut lines = Vec::new();
            let mut global_char_idx = 0;
            for line_str in full_text.split('\n') {
                let line_chars: Vec<char> = line_str.chars().collect();
                let line_len = line_chars.len();
                let line_start = global_char_idx;
                let line_end = line_start + line_len;

                if sel_end <= line_start || sel_start >= line_end {
                    lines.push(Line::from(line_str.to_string()));
                } else {
                    let mut spans = Vec::new();
                    let overlap_start = sel_start.max(line_start).saturating_sub(line_start);
                    let overlap_end = sel_end.min(line_end).saturating_sub(line_start);

                    if overlap_start > 0 {
                        let before: String = line_chars[..overlap_start].iter().collect();
                        spans.push(Span::raw(before));
                    }
                    if overlap_end > overlap_start {
                        let selected: String =
                            line_chars[overlap_start..overlap_end].iter().collect();
                        spans.push(Span::styled(
                            selected,
                            Style::default().add_modifier(Modifier::REVERSED),
                        ));
                    }
                    if overlap_end < line_len {
                        let after: String = line_chars[overlap_end..].iter().collect();
                        spans.push(Span::raw(after));
                    }
                    lines.push(Line::from(spans));
                }
                global_char_idx = line_end + 1;
            }
            Text::from(lines)
        } else {
            Text::from(display_content)
        };

        let mut input_widget = Paragraph::new(input_text).block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(Style::default().fg(BORDER_SUBTLE))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        if scroll_y > 0 {
            input_widget = input_widget.scroll((scroll_y, 0));
        }
        frame.render_widget(Clear, input_area);
        frame.render_widget(input_widget, input_area);

        let footer_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(38)])
            .split(footer_area);

        let cwd_widget = Paragraph::new(Line::from(vec![Span::styled(
            &self.cwd_display,
            Style::default().fg(TEXT_MUTED),
        )]));
        frame.render_widget(cwd_widget, footer_chunks[0]);

        let shortcuts_widget = Paragraph::new(Line::from(vec![Span::styled(
            "Ctrl+P: Palette · Ctrl+C: Interrupt",
            Style::default().fg(TEXT_MUTED),
        )]))
        .alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(shortcuts_widget, footer_chunks[1]);

        let cursor_x = input_area
            .x
            .saturating_add(2)
            .saturating_add(cur_col as u16);
        let cursor_y = input_area
            .y
            .saturating_add(1)
            .saturating_add(cur_line.saturating_sub(scroll_y as usize) as u16);
        if cursor_x > input_area.x
            && cursor_x
                < input_area
                    .x
                    .saturating_add(input_area.width.saturating_sub(2))
            && cursor_y > input_area.y
            && cursor_y
                < input_area
                    .y
                    .saturating_add(input_area.height.saturating_sub(1))
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        if self.popover.is_visible() {
            let item_count = self.popover.items.len().min(8) as u16;
            let popover_height = item_count + 2;
            let popover_width = (input_area.width.saturating_sub(4)).min(60);
            let popover_y = input_area.y.saturating_sub(popover_height);
            let popover_rect = Rect {
                x: input_area.x.saturating_add(2),
                y: popover_y,
                width: popover_width,
                height: popover_height,
            }
            .intersection(area);

            frame.render_widget(Clear, popover_rect);

            let max_visible = 8usize;
            let total = self.popover.items.len();
            let sel = self.popover.selected_idx;
            let start_idx = if total <= max_visible || sel < max_visible {
                0
            } else {
                (sel + 1).saturating_sub(max_visible)
            };

            let mut popover_lines = Vec::new();
            for (idx, item) in self
                .popover
                .items
                .iter()
                .enumerate()
                .skip(start_idx)
                .take(max_visible)
            {
                let is_sel = idx == self.popover.selected_idx;
                let (prefix, cmd_style, desc_style) = if is_sel {
                    (
                        " > ",
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                        Style::default().bg(Color::Cyan).fg(Color::Black),
                    )
                } else {
                    (
                        "   ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        Style::default().fg(TEXT_MUTED),
                    )
                };
                let mut spans = vec![Span::styled(prefix, desc_style)];
                if let Some(desc) = &item.description {
                    let cmd_col_w: usize = 14;
                    let pad = " ".repeat(cmd_col_w.saturating_sub(item.label.len()).max(1));
                    spans.push(Span::styled(&item.label, cmd_style));
                    spans.push(Span::styled(pad, desc_style));
                    spans.push(Span::styled(desc, desc_style));
                } else {
                    spans.push(Span::styled(&item.label, cmd_style));
                }
                popover_lines.push(Line::from(spans));
            }

            let title = match self.popover.kind {
                TriggerKind::Command => " Commands (Tab/Enter: apply, Esc: close) ",
                TriggerKind::File => " Files (Tab/Enter: apply, Esc: close) ",
            };

            let popover_widget = Paragraph::new(popover_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(popover_widget, popover_rect);
        }

        if !self.modals.is_empty() {
            self.modals.render(frame, area);
        }
    }
}
