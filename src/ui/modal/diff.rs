use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::diff::DiffViewer;
use crate::ui::modal::ModalAction;
use crate::ui::theme::BORDER_SUBTLE;

pub struct DiffModal {
    pub viewer: DiffViewer,
}

impl DiffModal {
    pub fn new(diff_text: &str) -> Self {
        Self {
            viewer: DiffViewer::new(diff_text),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                self.viewer.scroll_up(1);
                ModalAction::None
            }
            KeyCode::Down => {
                self.viewer.scroll_down(1);
                ModalAction::None
            }
            KeyCode::PageUp => {
                self.viewer.scroll_up(10);
                ModalAction::None
            }
            KeyCode::PageDown => {
                self.viewer.scroll_down(10);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let total = self.viewer.line_count();
        let offset = self.viewer.scroll_offset;
        let title = format!(
            " Integrated Diff Viewer [Line {}/{} total] (Up/Down/PgUp/PgDn: Scroll, Esc: Close) ",
            if total == 0 { 0 } else { offset + 1 },
            total
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .title(title);

        let inner_height = area.height.saturating_sub(2) as usize;
        let lines = self.viewer.render_lines(inner_height);

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
