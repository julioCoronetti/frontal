use std::ops::{Deref, DerefMut};

use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::ui::modal::select::{DialogSelect, SelectItem, SelectResult};
use crate::ui::modal::ModalAction;

const COMMAND_DEFINITIONS: &[(&str, &str, &str)] = &[
    ("new_tab", "New Session Tab", "Ctrl+T"),
    ("close_tab", "Close Session Tab", "Ctrl+W"),
    ("select_model", "Select Model & Effort", "/model"),
    ("select_mode", "Select Execution Mode", "/mode"),
    ("resume_session", "Resume Past Session", "/resume"),
    ("rename_session", "Rename Current Session", "/rename"),
    ("view_usage", "View Model Quotas", "/quota"),
    ("toggle_sidebar", "Toggle Context Sidebar", "Ctrl+B"),
    ("clear", "Clear Conversation Viewport", "/clear"),
    ("quit", "Quit Frontal", "Ctrl+Q"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandsModal {
    pub select: DialogSelect<String>,
}

impl Deref for CommandsModal {
    type Target = DialogSelect<String>;

    fn deref(&self) -> &Self::Target {
        &self.select
    }
}

impl DerefMut for CommandsModal {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.select
    }
}

impl Default for CommandsModal {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandsModal {
    pub fn new() -> Self {
        let select_items: Vec<SelectItem<String>> = COMMAND_DEFINITIONS
            .iter()
            .map(|&(id, title, shortcut)| {
                SelectItem::new(id, title, id.to_string()).with_shortcut(shortcut)
            })
            .collect();

        Self {
            select: DialogSelect::new("Commands", select_items),
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        self.select.filtered_indices()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match self.select.handle_key(key) {
            SelectResult::Selected(item) => ModalAction::ExecuteCommand(item.value),
            SelectResult::Cancel => ModalAction::Close,
            SelectResult::None | SelectResult::Deleted(_) => ModalAction::None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> ModalAction {
        match self.select.handle_mouse(mouse) {
            SelectResult::Selected(item) => ModalAction::ExecuteCommand(item.value),
            SelectResult::Cancel => ModalAction::Close,
            SelectResult::None | SelectResult::Deleted(_) => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        self.select.render(frame, area);
    }
}
