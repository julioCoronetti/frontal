use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::security::DangerAssessment;
use crate::ui::modal::ModalAction;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DangerConfirmationModal {
    pub assessment: DangerAssessment,
}

impl DangerConfirmationModal {
    pub fn new(assessment: DangerAssessment) -> Self {
        Self { assessment }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Enter => ModalAction::ApproveDanger,
            KeyCode::Esc => ModalAction::RejectDanger,
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width < 4 || area.height < 4 {
            frame.render_widget(Clear, area);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT_RUBY))
            .style(Style::default().bg(Color::Reset));

        let mut lines = Vec::new();
        let header_title = format!(" SECURITY GUARDIAN — {} ", self.assessment.category.title());
        let esc_pad = (area.width as usize).saturating_sub(header_title.chars().count() + 6);

        lines.push(Line::from(vec![
            Span::styled(
                header_title,
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>esc_pad$}", "esc: abort "),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        lines.push(Line::raw(""));

        lines.push(Line::from(vec![
            Span::styled(" Warning: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                &self.assessment.warning,
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled(" Target:  ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                &self.assessment.target,
                Style::default().fg(ACCENT_HIGHLIGHT),
            ),
        ]));

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            " Command to execute:",
            Style::default().fg(TEXT_MUTED),
        )]));

        let max_w = (area.width as usize).saturating_sub(6);
        let cmd_preview = if self.assessment.command.len() > max_w {
            let limit = max_w.saturating_sub(3);
            format!("   {}...", &self.assessment.command[..limit])
        } else {
            format!("   {}", self.assessment.command)
        };
        lines.push(Line::from(Span::styled(
            cmd_preview,
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(
                " [Enter] ",
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Approve and proceed   ", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(
                " [Esc] ",
                Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Block and abort", Style::default().fg(TEXT_MUTED)),
        ]));

        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
