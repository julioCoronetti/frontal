use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};

use crate::ui::modal::ModalAction;

use super::{UsageModal, UsagePage};

impl UsageModal {
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self
            .scroll_offset
            .saturating_sub(n)
            .min(self.max_scroll.get());
    }

    pub fn scroll_down(&mut self, n: usize) {
        let limit = self.max_scroll.get();
        self.scroll_offset = (self.scroll_offset + n).min(limit);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => {
                if self.page == UsagePage::OpenCodeStats {
                    self.page = UsagePage::Main;
                    self.scroll_offset = 0;
                    self.max_scroll.set(0);
                    ModalAction::None
                } else {
                    ModalAction::Close
                }
            }
            KeyCode::Enter => {
                if self.is_opencode && self.page == UsagePage::Main {
                    self.open_stats_subpage();
                    ModalAction::None
                } else if self.page == UsagePage::OpenCodeStats {
                    ModalAction::None
                } else {
                    ModalAction::Close
                }
            }
            KeyCode::Char('q') => ModalAction::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                ModalAction::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
                ModalAction::None
            }
            KeyCode::PageUp => {
                self.scroll_up(5);
                ModalAction::None
            }
            KeyCode::PageDown => {
                self.scroll_down(5);
                ModalAction::None
            }
            KeyCode::Home => {
                self.scroll_offset = 0;
                ModalAction::None
            }
            KeyCode::End => {
                self.scroll_offset = self.max_scroll.get();
                ModalAction::None
            }
            KeyCode::Char('r') => {
                self.refresh();
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> ModalAction {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(2);
                ModalAction::None
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(2);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }
}
