use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::Frame;

use crate::ui::theme::{BACKDROP_BG, BACKDROP_FG};

pub mod danger;
pub mod diff;
pub mod model_effort;
pub mod palette;
pub mod respawn;
pub mod select;

pub use danger::DangerConfirmationModal;
pub use diff::DiffModal;
pub use model_effort::{ModelEffortModal, ModelEffortStep, ModelItem};
pub use palette::{CommandPaletteModal, PaletteItem};
pub use respawn::HotRespawnModal;
pub use select::{DialogSelect, SelectItem, SelectResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalAction {
    None,
    Close,
    ApproveDanger,
    RejectDanger,
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
    SwitchTab(usize),
    ExecuteCommand(String),
}

pub enum Modal {
    ModelEffort(ModelEffortModal),
    HotRespawn(HotRespawnModal),
    CommandPalette(CommandPaletteModal),
    Diff(DiffModal),
    Select(DialogSelect<ModalAction>),
    DangerConfirmation(DangerConfirmationModal),
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match self {
            Self::ModelEffort(m) => m.handle_key(key),
            Self::HotRespawn(m) => m.handle_key(key),
            Self::CommandPalette(m) => m.handle_key(key),
            Self::Diff(m) => m.handle_key(key),
            Self::Select(m) => match m.handle_key(key) {
                SelectResult::Selected(item) => item.value.clone(),
                SelectResult::Cancel => ModalAction::Close,
                SelectResult::None => ModalAction::None,
            },
            Self::DangerConfirmation(m) => m.handle_key(key),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Self::ModelEffort(m) => m.render(frame, area),
            Self::HotRespawn(m) => m.render(frame, area),
            Self::CommandPalette(m) => m.render(frame, area),
            Self::Diff(m) => m.render(frame, area),
            Self::Select(m) => m.render(frame, area),
            Self::DangerConfirmation(m) => m.render(frame, area),
        }
    }

    pub fn popup_area(&self, screen: Rect) -> Rect {
        match self {
            Self::Diff(_) => ModalStack::centered_rect_bounded(85, 75, 50, 160, 14, 40, screen),
            Self::ModelEffort(_) => {
                ModalStack::centered_rect_bounded(46, 52, 42, 56, 14, 20, screen)
            }
            Self::CommandPalette(_) | Self::Select(_) => {
                ModalStack::centered_rect_bounded(48, 55, 42, 64, 12, 22, screen)
            }
            Self::HotRespawn(_) => {
                ModalStack::centered_rect_bounded(52, 58, 44, 68, 14, 24, screen)
            }
            Self::DangerConfirmation(m) => {
                let cmd_len = m.assessment.command.chars().count() + 6;
                let warn_len = m.assessment.warning.chars().count() + 12;
                let target_len = m.assessment.target.chars().count() + 12;
                let w = (cmd_len.max(warn_len).max(target_len).max(58) as u16).min(76);
                ModalStack::centered_rect_bounded(40, 32, w, w, 11, 12, screen)
            }
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

    pub fn remove_danger_modal(&mut self) {
        self.stack
            .retain(|m| !matches!(m, Modal::DangerConfirmation(_)));
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
            Self::render_backdrop(frame, screen_area);
            let popup_area = top.popup_area(screen_area);
            top.render(frame, popup_area);
        }
    }

    fn render_backdrop(frame: &mut Frame, area: Rect) {
        let area = area.intersection(frame.area());
        let buf = frame.buffer_mut();
        let backdrop_style = Style::default()
            .add_modifier(Modifier::DIM)
            .bg(BACKDROP_BG)
            .fg(BACKDROP_FG);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_style(backdrop_style);
            }
        }
    }

    pub fn centered_rect_bounded(
        percent_x: u16,
        percent_y: u16,
        min_w: u16,
        max_w: u16,
        min_h: u16,
        max_h: u16,
        r: Rect,
    ) -> Rect {
        let raw_w = (r.width as u32 * percent_x as u32 / 100) as u16;
        let raw_h = (r.height as u32 * percent_y as u32 / 100) as u16;
        let width = raw_w.clamp(min_w, max_w).min(r.width);
        let height = raw_h.clamp(min_h, max_h).min(r.height);
        let x = r.x + (r.width.saturating_sub(width)) / 2;
        let y = r.y + (r.height.saturating_sub(height)) / 2;
        Rect::new(x, y, width, height)
    }

    pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        Self::centered_rect_bounded(percent_x, percent_y, 0, r.width, 0, r.height, r)
    }
}

impl Default for ModalStack {
    fn default() -> Self {
        Self::new()
    }
}
