mod chrome;
mod input;
mod panes;
mod welcome;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Color;
use ratatui::widgets::Clear;
use ratatui::Frame;

use crate::core::model_spec::ContextSemaphore;
use crate::ui::app::App;

const SIDEBAR_WIDTH: u16 = 38;
const SIDEBAR_GAP: u16 = 2;
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn semaphore_color(sem: ContextSemaphore) -> Color {
    match sem {
        ContextSemaphore::Green => Color::Green,
        ContextSemaphore::Yellow => Color::Yellow,
        ContextSemaphore::Red => Color::Red,
    }
}

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
        let main_width = if show_sidebar {
            padded_area
                .width
                .saturating_sub(SIDEBAR_WIDTH + SIDEBAR_GAP)
        } else {
            padded_area.width
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(3),
            ])
            .split(padded_area);

        let tab_bar_area = chunks[0];
        let divider_area = chunks[1];
        let body_area = Rect {
            x: padded_area.x,
            y: chunks[2].y,
            width: main_width,
            height: chunks[2].height,
        };

        let spinner_char = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];

        self.render_tab_bar(frame, tab_bar_area, spinner_char);
        self.render_divider(frame, divider_area, padded_area.width);

        if self.is_hero_welcome_active(area) {
            let hero_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(body_area);
            let hero_area = hero_chunks[0];
            let footer_area = hero_chunks[1];

            self.viewport_bounds = (hero_area.x, hero_area.y, hero_area.width, hero_area.height);
            let input_area = self.render_welcome_hero(frame, hero_area);
            self.render_version_footer(frame, footer_area);
            self.render_popover(frame, input_area, area, true);
        } else {
            let inner_w = main_width.saturating_sub(4) as usize;
            let input_lines =
                (input::visual_line_count(&self.editor.text(), inner_w) as u16).clamp(1, 8);
            let bottom_height = input_lines + 3;
            let body_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(bottom_height)])
                .split(body_area);

            let viewport_area = body_chunks[0];
            let bottom_area = body_chunks[1];

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

            let sidebar_area = if show_sidebar {
                Some(Rect {
                    x: padded_area.x.saturating_add(main_width + SIDEBAR_GAP),
                    y: padded_area.y.saturating_add(2),
                    width: padded_area.width.saturating_sub(main_width + SIDEBAR_GAP),
                    height: padded_area.height.saturating_sub(2),
                })
            } else {
                None
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

            self.render_footer(frame, footer_area, show_sidebar);
            self.render_popover(frame, input_area, area, false);
        }

        if !self.modals.is_empty() {
            self.modals.render(frame, area);
        }
    }
}
