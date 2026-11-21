use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::modal::select::{DialogSelect, SelectItem, SelectResult};
use crate::ui::modal::ModalAction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandItem {
    pub id: String,
    pub title: String,
    pub shortcut: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandsModal {
    pub select: DialogSelect<String>,
    pub query: String,
    pub items: Vec<CommandItem>,
    pub selected_idx: usize,
}

impl Default for CommandsModal {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandsModal {
    pub fn new() -> Self {
        let command_items = vec![
            CommandItem {
                id: "new_tab".to_string(),
                title: "New Session Tab".to_string(),
                shortcut: "Ctrl+T".to_string(),
            },
            CommandItem {
                id: "close_tab".to_string(),
                title: "Close Session Tab".to_string(),
                shortcut: "Ctrl+W".to_string(),
            },
            CommandItem {
                id: "select_model".to_string(),
                title: "Select Model & Effort".to_string(),
                shortcut: "/model".to_string(),
            },
            CommandItem {
                id: "select_mode".to_string(),
                title: "Select Execution Mode".to_string(),
                shortcut: "/mode".to_string(),
            },
            CommandItem {
                id: "resume_session".to_string(),
                title: "Resume Past Session".to_string(),
                shortcut: "/resume".to_string(),
            },
            CommandItem {
                id: "rename_session".to_string(),
                title: "Rename Current Session".to_string(),
                shortcut: "/rename".to_string(),
            },
            CommandItem {
                id: "view_usage".to_string(),
                title: "View Model Quotas".to_string(),
                shortcut: "/quota".to_string(),
            },
            CommandItem {
                id: "toggle_sidebar".to_string(),
                title: "Toggle Context Sidebar".to_string(),
                shortcut: "Ctrl+B".to_string(),
            },
            CommandItem {
                id: "clear".to_string(),
                title: "Clear Conversation Viewport".to_string(),
                shortcut: "/clear".to_string(),
            },
            CommandItem {
                id: "quit".to_string(),
                title: "Quit Frontal".to_string(),
                shortcut: "Ctrl+Q".to_string(),
            },
        ];

        let select_items: Vec<SelectItem<String>> = command_items
            .iter()
            .map(|it| {
                SelectItem::new(it.id.clone(), it.title.clone(), it.id.clone())
                    .with_shortcut(it.shortcut.clone())
            })
            .collect();

        let select = DialogSelect::new("Commands", select_items);

        Self {
            select,
            query: String::new(),
            items: command_items,
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
            SelectResult::None | SelectResult::Deleted(_) => ModalAction::None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> ModalAction {
        self.select.query = self.query.clone();
        self.select.selected_idx = self.selected_idx;
        let res = self.select.handle_mouse(mouse);
        self.query = self.select.query.clone();
        self.selected_idx = self.select.selected_idx;
        match res {
            SelectResult::Selected(item) => ModalAction::ExecuteCommand(item.value),
            SelectResult::Cancel => ModalAction::Close,
            SelectResult::None | SelectResult::Deleted(_) => ModalAction::None,
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
            self.select.last_area.set(select.last_area.get());
            self.select.scroll_offset.set(select.scroll_offset.get());
        }
    }
}
