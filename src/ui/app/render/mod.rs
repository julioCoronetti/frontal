mod chrome;
mod panes;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Clear;
use ratatui::Frame;

use crate::ui::app::App;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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
        let divider_area = chunks[1];
        let viewport_area = chunks[2];
        let input_area = chunks[3];
        let footer_area = chunks[4];

        let spinner_char = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];

        self.render_tab_bar(frame, tab_bar_area, spinner_char);
        self.render_divider(frame, divider_area, padded_area.width);

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

        self.render_chat(frame, chat_area, is_welcome, spinner_char);
        if let Some(sb_area) = sidebar_area {
            self.render_sidebar(frame, sb_area);
        }

        if let Some((cursor_x, cursor_y)) = self.render_input(frame, input_area) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        self.render_footer(frame, footer_area);
        self.render_popover(frame, input_area, area);

        if !self.modals.is_empty() {
            self.modals.render(frame, area);
        }
    }
}
