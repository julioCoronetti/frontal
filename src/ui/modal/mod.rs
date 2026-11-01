use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::Frame;

use crate::ui::theme::{BACKDROP_BG, BACKDROP_FG};

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

    pub fn popup_area(&self, screen: Rect) -> Rect {
        match self {
            Self::Diff(_) => ModalStack::centered_rect_bounded(85, 75, 50, 160, 14, 40, screen),
            Self::ModelEffort(_) => {
                ModalStack::centered_rect_bounded(48, 55, 42, 64, 12, 22, screen)
            }
            Self::CommandPalette(_) => {
                ModalStack::centered_rect_bounded(48, 55, 42, 64, 12, 22, screen)
            }
            Self::HotRespawn(_) => {
                ModalStack::centered_rect_bounded(52, 58, 44, 68, 14, 24, screen)
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
