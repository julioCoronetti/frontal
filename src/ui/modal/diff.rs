use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::diff::DiffViewer;
use crate::ui::modal::ModalAction;
use crate::ui::theme::{BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

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
        if area.width < 4 || area.height < 4 {
            frame.render_widget(Clear, area);
            return;
        }

        let total = self.viewer.line_count();
        let offset = self.viewer.scroll_offset;

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE));

        let mut lines = Vec::new();
        let title = " Integrated Diff Viewer:";
        let esc_pad = (area.width as usize).saturating_sub(title.len() + 5);
        lines.push(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>esc_pad$}", "esc "),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        lines.push(Line::raw(""));

        let line_info = format!(
            "   Viewing: Line {}/{} total",
            if total == 0 { 0 } else { offset + 1 },
            total
        );
        let nav_hint = "↑/↓/PgUp/PgDn: Scroll ";
        let nav_pad = (area.width as usize).saturating_sub(line_info.len() + nav_hint.len() + 2);
        lines.push(Line::from(vec![
            Span::styled(line_info, Style::default().fg(TEXT_PRIMARY)),
            Span::styled(
                format!("{:>nav_pad$}{}", "", nav_hint),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        let line_w = (area.width as usize).saturating_sub(6);
        let divider = "─".repeat(line_w);
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", divider),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        lines.push(Line::raw(""));

        let inner_height = area.height.saturating_sub(7) as usize;
        let diff_lines = self.viewer.render_lines(inner_height);
        if diff_lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "   No local Git changes or diff content to display",
                Style::default().fg(TEXT_MUTED),
            )]));
        } else {
            for diff_line in diff_lines {
                let mut spans = vec![Span::raw("   ")];
                spans.extend(diff_line.spans);
                lines.push(Line::from(spans));
            }
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
