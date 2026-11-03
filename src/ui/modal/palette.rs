use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::modal::select::{DialogSelect, SelectItem, SelectResult};
use crate::ui::modal::ModalAction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub id: String,
    pub title: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPaletteModal {
    pub select: DialogSelect<String>,
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
        let palette_items = vec![
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
        ];

        let select_items: Vec<SelectItem<String>> = palette_items
            .iter()
            .map(|it| {
                SelectItem::new(it.id.clone(), it.title.clone(), it.id.clone())
                    .with_shortcut(it.shortcut.clone())
            })
            .collect();

        let select = DialogSelect::new("Command Palette (Enter: Run, Esc: Close)", select_items);

        Self {
            select,
            query: String::new(),
            items: palette_items,
            selected_idx: 0,
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        if self.query == self.select.query {
            self.select.filtered_indices()
        } else {
            let mut select = self.select.clone();
            select.query = self.query.clone();
            select.filtered_indices()
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        self.select.query = self.query.clone();
        self.select.selected_idx = self.selected_idx;
        let res = self.select.handle_key(key);
        self.query = self.select.query.clone();
        self.selected_idx = self.select.selected_idx;
        match res {
            SelectResult::Selected(item) => ModalAction::ExecuteCommand(item.value),
            SelectResult::Cancel => ModalAction::Close,
            SelectResult::None => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if self.query == self.select.query && self.selected_idx == self.select.selected_idx {
            self.select.render(frame, area);
        } else {
            let mut select = self.select.clone();
            select.query = self.query.clone();
            select.selected_idx = self.selected_idx;
            select.render(frame, area);
        }
    }
}
