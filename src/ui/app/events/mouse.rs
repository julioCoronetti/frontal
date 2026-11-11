use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::ui::app::{App, AppAction};

impl App {
    pub(crate) fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<AppAction> {
        if !self.modals.is_empty() {
            let action = self.modals.handle_mouse_event(mouse);
            self.dirty = true;
            if let Some(res) = self.dispatch_modal_action(action) {
                return res;
            }
            return None;
        }
        let (column, row) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.viewport.scroll_up(3);
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                self.viewport.scroll_down(3);
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.editor.has_selection() {
                    self.editor.clear_selection();
                    self.dirty = true;
                }
                let (vx, vy, vw, vh) = self.viewport_bounds;
                if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                    let offset = self.viewport.effective_offset();
                    let line = offset + (row.saturating_sub(vy) as usize);
                    let col = column.saturating_sub(vx) as usize;
                    self.viewport.start_selection(line, col);
                    self.dirty = true;
                } else if self.viewport.selection.is_some() {
                    self.viewport.clear_selection();
                    self.dirty = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let (vx, vy, vw, vh) = self.viewport_bounds;
                if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                    let offset = self.viewport.effective_offset();
                    let line = offset + (row.saturating_sub(vy) as usize);
                    let col = column.saturating_sub(vx) as usize;
                    self.viewport.update_selection(line, col);
                    self.dirty = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if matches!(self.viewport.selection, Some((s, e)) if s == e) {
                    self.viewport.clear_selection();
                    let (vx, vy, vw, vh) = self.viewport_bounds;
                    if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                        let offset = self.viewport.effective_offset();
                        let line = offset + (row.saturating_sub(vy) as usize);
                        if self.cache.toggle_collapsible_at_line(line).is_some() {
                            self.dirty = true;
                        }
                    }
                }
                self.dirty = true;
            }
            _ => {}
        }
        None
    }
}
