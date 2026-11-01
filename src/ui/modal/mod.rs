use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

pub mod diff;
pub mod model_effort;
pub mod palette;
pub mod respawn;

pub use diff::DiffModal;
pub use model_effort::{ModelEffortModal, ModelEffortStep, ModelItem};
pub use palette::{CommandPaletteModal, PaletteItem};
pub use respawn::HotRespawnModal;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalAction {
    None,
    Close,
    SelectModelEffort {
        model: String,
        effort: String,
    },
    Respawn {
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    },
    ExecuteCommand(String),
}

pub enum Modal {
    ModelEffort(ModelEffortModal),
    HotRespawn(HotRespawnModal),
    CommandPalette(CommandPaletteModal),
    Diff(DiffModal),
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match self {
            Self::ModelEffort(m) => m.handle_key(key),
            Self::HotRespawn(m) => m.handle_key(key),
            Self::CommandPalette(m) => m.handle_key(key),
            Self::Diff(m) => m.handle_key(key),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Self::ModelEffort(m) => m.render(frame, area),
            Self::HotRespawn(m) => m.render(frame, area),
            Self::CommandPalette(m) => m.render(frame, area),
            Self::Diff(m) => m.render(frame, area),
        }
    }
}

pub struct ModalStack {
    pub stack: Vec<Modal>,
}

impl ModalStack {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn push(&mut self, modal: Modal) {
        self.stack.push(modal);
    }

    pub fn pop(&mut self) -> Option<Modal> {
        self.stack.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn top_mut(&mut self) -> Option<&mut Modal> {
        self.stack.last_mut()
    }

    pub fn top(&self) -> Option<&Modal> {
        self.stack.last()
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> ModalAction {
        if let Some(top) = self.stack.last_mut() {
            let act = top.handle_key(key);
            if act == ModalAction::Close {
                self.stack.pop();
            }
            act
        } else {
            ModalAction::None
        }
    }

    pub fn render(&self, frame: &mut Frame, screen_area: Rect) {
        if let Some(top) = self.stack.last() {
            let popup_area = Self::centered_rect(75, 60, screen_area);
            top.render(frame, popup_area);
        }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}

impl Default for ModalStack {
    fn default() -> Self {
        Self::new()
    }
}
