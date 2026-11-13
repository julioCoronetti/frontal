mod chrome;
mod panes;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::widgets::Clear;
use ratatui::Frame;

use crate::ui::app::App;

const SIDEBAR_WIDTH: u16 = 38;
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

        let is_welcome = self.cache.blocks_count() == 0 && !self.waiting_for_response;
        let show_sidebar = !is_welcome && self.sidebar_visible && padded_area.width >= 100;
        let (main_column, sidebar_area) = if show_sidebar {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(60), Constraint::Length(SIDEBAR_WIDTH)])
                .split(padded_area);
            (cols[0], Some(cols[1]))
        } else {
            (padded_area, None)
        };

        let input_lines = (self.editor.line_count() as u16).clamp(1, 8);
        let bottom_height = input_lines + 3;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(bottom_height),
            ])
            .split(main_column);

        let tab_bar_area = chunks[0];
        let divider_area = chunks[1];
        let viewport_area = chunks[2];
        let bottom_area = chunks[3];

        let spinner_char = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];

        self.render_tab_bar(frame, tab_bar_area, spinner_char);
        self.render_divider(frame, divider_area, divider_area.width);

        let input_height = (input_lines + 2).min(bottom_area.height);
        let input_area = Rect {
            x: bottom_area.x,
            y: bottom_area.y,
            width: bottom_area.width,
            height: input_height,
        };
        let footer_area = Rect {
            x: bottom_area.x,
            y: bottom_area.y.saturating_add(input_height),
            width: bottom_area.width,
            height: bottom_area.height.saturating_sub(input_height).min(1),
        };

        self.viewport_bounds = (
            viewport_area.x,
            viewport_area.y,
            viewport_area.width,
            viewport_area.height,
        );

        self.render_chat(frame, viewport_area, is_welcome, spinner_char);
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
