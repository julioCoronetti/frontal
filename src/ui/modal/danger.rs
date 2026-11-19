use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

fn sanitize_and_truncate(raw: &str, max_w: usize) -> String {
    if max_w == 0 {
        return String::new();
    }
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .filter(|c| !c.is_control())
        .collect();
    if cleaned.chars().count() <= max_w {
        return cleaned;
    }
    if max_w > 3 {
        let mut out: String = cleaned.chars().take(max_w - 3).collect();
        out.push_str("...");
        out
    } else {
        cleaned.chars().take(max_w).collect()
    }
}

impl DangerConfirmationModal {
    pub fn new(assessment: DangerAssessment) -> Self {
        Self { assessment }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        if matches!(key.code, KeyCode::Char('c' | 'C'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            return ModalAction::RejectDanger;
        }
        match key.code {
            KeyCode::Enter if key.modifiers.is_empty() => ModalAction::ApproveDanger,
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

        lines.push(Line::from(Span::styled(
            header_title,
            Style::default()
                .fg(ACCENT_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw(""));

        let field_budget = (area.width as usize).saturating_sub(12);
        lines.push(Line::from(vec![
            Span::styled(" Warning: ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                sanitize_and_truncate(&self.assessment.warning, field_budget),
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled(" Target:  ", Style::default().fg(TEXT_MUTED)),
            Span::styled(
                sanitize_and_truncate(&self.assessment.target, field_budget),
                Style::default().fg(ACCENT_HIGHLIGHT),
            ),
        ]));

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![Span::styled(
            " Command to execute:",
            Style::default().fg(TEXT_MUTED),
        )]));

        let max_w = (area.width as usize).saturating_sub(6);
        let cmd_preview = format!(
            "   {}",
            sanitize_and_truncate(&self.assessment.command, max_w)
        );
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
