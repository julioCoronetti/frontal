use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::modal::ModalAction;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

pub struct HotRespawnModal {
    pub conversation_id: String,
    pub model: String,
    pub effort: String,
    pub mode: String,
    pub active_field: usize,
}

impl HotRespawnModal {
    pub fn new(
        conv_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> Self {
        Self {
            conversation_id: conv_id.unwrap_or_default(),
            model: model.unwrap_or_else(|| crate::config::DEFAULT_MODEL.to_string()),
            effort: effort.unwrap_or_else(|| crate::config::DEFAULT_EFFORT.to_string()),
            mode: mode.unwrap_or_else(|| crate::config::DEFAULT_MODE.to_string()),
            active_field: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Tab | KeyCode::Down => {
                self.active_field = (self.active_field + 1) % 4;
                ModalAction::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.active_field = (self.active_field + 3) % 4;
                ModalAction::None
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let target = match self.active_field {
                    0 => &mut self.conversation_id,
                    1 => &mut self.model,
                    2 => &mut self.effort,
                    _ => &mut self.mode,
                };
                target.push(c);
                ModalAction::None
            }
            KeyCode::Backspace => {
                let target = match self.active_field {
                    0 => &mut self.conversation_id,
                    1 => &mut self.model,
                    2 => &mut self.effort,
                    _ => &mut self.mode,
                };
                target.pop();
                ModalAction::None
            }
            KeyCode::Enter => {
                let conv = if self.conversation_id.trim().is_empty() {
                    None
                } else {
                    Some(self.conversation_id.trim().to_string())
                };
                let mod_val = if self.model.trim().is_empty() {
                    None
                } else {
                    Some(self.model.trim().to_string())
                };
                let eff = if self.effort.trim().is_empty() {
                    None
                } else {
                    Some(self.effort.trim().to_string())
                };
                let md = if self.mode.trim().is_empty() {
                    None
                } else {
                    Some(self.mode.trim().to_string())
                };

                ModalAction::Respawn {
                    conversation_id: conv,
                    model: mod_val,
                    effort: eff,
                    mode: md,
                }
            }
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
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .style(Style::default().bg(Color::Reset));

        let mut lines = Vec::new();
        let title = " Hot-Respawn Session:";
        let esc_pad = (area.width as usize).saturating_sub(title.chars().count() + 5);
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

        let param_title = "   Session Configuration:";
        let nav_hint = "Tab/Shift+Tab: Navigate  Enter: Run ";
        let nav_pad = (area.width as usize)
            .saturating_sub(param_title.chars().count() + nav_hint.chars().count() + 5);
        lines.push(Line::from(vec![
            Span::styled(param_title, Style::default().fg(TEXT_PRIMARY)),
            Span::styled(
                format!("{:>nav_pad$}{}", "", nav_hint),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        let line_w = (area.width as usize).saturating_sub(8);
        let divider = "─".repeat(line_w);
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", divider),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        lines.push(Line::raw(""));

        let labels = [
            "Conversation UUID:",
            "Model ID:",
            "Reasoning Effort:",
            "Execution Mode:",
        ];
        let values = [&self.conversation_id, &self.model, &self.effort, &self.mode];

        for i in 0..4 {
            let is_active = i == self.active_field;
            let bullet = if is_active { " ● " } else { "   " };
            let bullet_style = Style::default().fg(if is_active {
                ACCENT_HIGHLIGHT
            } else {
                TEXT_MUTED
            });
            let field_style = if is_active {
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_MUTED)
            };
            let val_style = if is_active {
                Style::default()
                    .bg(ACCENT_RUBY)
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            let cursor = if is_active { "█" } else { " " };

            lines.push(Line::from(vec![
                Span::styled(bullet, bullet_style),
                Span::styled(format!("{:<20} ", labels[i]), field_style),
                Span::styled(values[i].clone(), val_style),
                Span::styled(cursor, Style::default().fg(ACCENT_HIGHLIGHT)),
            ]));
            lines.push(Line::raw(""));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
