use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::modal::ModalAction;
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

pub struct PaletteItem {
    pub id: String,
    pub title: String,
    pub shortcut: String,
}

pub struct CommandPaletteModal {
    pub query: String,
    pub items: Vec<PaletteItem>,
    pub selected_idx: usize,
}

impl Default for CommandPaletteModal {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPaletteModal {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            items: vec![
                PaletteItem {
                    id: "new_tab".to_string(),
                    title: "New Session Tab".to_string(),
                    shortcut: "Ctrl+T".to_string(),
                },
                PaletteItem {
                    id: "close_tab".to_string(),
                    title: "Close Session Tab".to_string(),
                    shortcut: "Ctrl+W".to_string(),
                },
                PaletteItem {
                    id: "select_model".to_string(),
                    title: "Select Model & Effort".to_string(),
                    shortcut: "/model".to_string(),
                },
                PaletteItem {
                    id: "hot_respawn".to_string(),
                    title: "Hot-Respawn Session".to_string(),
                    shortcut: "/respawn".to_string(),
                },
                PaletteItem {
                    id: "view_diff".to_string(),
                    title: "View Git/Patch Diff".to_string(),
                    shortcut: "/diff".to_string(),
                },
                PaletteItem {
                    id: "clear".to_string(),
                    title: "Clear Conversation Viewport".to_string(),
                    shortcut: "/clear".to_string(),
                },
                PaletteItem {
                    id: "interrupt".to_string(),
                    title: "Interrupt Running Turn".to_string(),
                    shortcut: "Ctrl+C".to_string(),
                },
                PaletteItem {
                    id: "quit".to_string(),
                    title: "Quit Frontal".to_string(),
                    shortcut: "Ctrl+Q".to_string(),
                },
            ],
            selected_idx: 0,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                if q.is_empty() {
                    true
                } else {
                    it.title.to_lowercase().contains(&q) || it.shortcut.to_lowercase().contains(&q)
                }
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        let filtered = self.filtered_indices();
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                ModalAction::None
            }
            KeyCode::Down => {
                if !filtered.is_empty() && self.selected_idx + 1 < filtered.len() {
                    self.selected_idx += 1;
                }
                ModalAction::None
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.selected_idx = 0;
                ModalAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_idx = 0;
                ModalAction::None
            }
            KeyCode::Enter => {
                if let Some(&item_idx) = filtered.get(self.selected_idx) {
                    let cmd_id = self.items[item_idx].id.clone();
                    ModalAction::ExecuteCommand(cmd_id)
                } else {
                    ModalAction::None
                }
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .title(" Command Palette (Enter: Run, Esc: Close) ");

        let filtered = self.filtered_indices();
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                " > ",
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}_", self.query),
                Style::default().fg(TEXT_PRIMARY),
            ),
        ]));
        lines.push(Line::raw(""));

        for (pos, &idx) in filtered.iter().enumerate() {
            let item = &self.items[idx];
            let is_sel = pos == self.selected_idx;
            let style = if is_sel {
                Style::default()
                    .bg(ACCENT_RUBY)
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_PRIMARY)
            };
            let prefix = if is_sel { " * " } else { "   " };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{:<30}", item.title), style),
                Span::styled(item.shortcut.clone(), Style::default().fg(TEXT_MUTED)),
            ]));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
