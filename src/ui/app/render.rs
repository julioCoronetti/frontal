use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
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

        let total_lines = self.cache.ensure_visual_lines(viewport_area.width);
        let total_with_spinner = total_lines + if self.waiting_for_response { 1 } else { 0 };
        self.viewport
            .sync_dimensions(total_with_spinner, viewport_area.height as usize);
        let mut visible_lines = self.viewport.visible_slice(&self.cache);

        if self.waiting_for_response {
            const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];
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
        frame.render_widget(viewport_widget, viewport_area);

        let model_name = self.active_model_name();
        let mode_name = self.active_mode_name();
        let effort_name = self.active_effort_name();
        let input_title = if model_name.ends_with(&format!("-{}", effort_name)) {
            format!(" Input [{}] · {} ", mode_name, model_name)
        } else {
            format!(" Input [{}] · {} ({}) ", mode_name, model_name, effort_name)
        };
        let display_content = if !self.editor.is_empty() {
            self.editor.text()
        } else {
            self.input.content().to_string()
        };

        let (cur_line, cur_col) = if !self.editor.is_empty() {
            self.editor.cursor_coordinates()
        } else {
            (0, self.input.cursor_char_idx())
        };

        let max_visible = input_area.height.saturating_sub(2) as usize;
        let scroll_y = if max_visible > 0 && cur_line >= max_visible {
            (cur_line + 1 - max_visible) as u16
        } else {
            0
        };

        let mut input_widget = Paragraph::new(display_content).block(
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

            let mut popover_lines = Vec::new();
            for (idx, item) in self
                .popover
                .items
                .iter()
                .enumerate()
                .take(item_count as usize)
            {
                let is_sel = idx == self.popover.selected_idx;
                let (prefix, style) = if is_sel {
                    (
                        " > ",
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("   ", Style::default().fg(Color::White))
                };
                popover_lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&item.label, style),
                ]));
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
