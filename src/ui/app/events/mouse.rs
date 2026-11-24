use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

use crate::ui::app::{App, AppAction, HitAction};

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
            MouseEventKind::Moved => {
                let mut hovered_tab = None;
                let mut hovered_new = false;
                for region in &self.hit_regions {
                    if !region.contains(column, row) {
                        continue;
                    }
                    match region.action {
                        HitAction::SwitchTab(idx) | HitAction::CloseOrResetTab(idx) => {
                            hovered_tab = Some(idx);
                            break;
                        }
                        HitAction::NewTab => {
                            hovered_new = true;
                            break;
                        }
                    }
                }
                if hovered_tab != self.hovered_tab_index || hovered_new != self.hovered_new_tab {
                    self.hovered_tab_index = hovered_tab;
                    self.hovered_new_tab = hovered_new;
                    self.dirty = true;
                }
            }
            MouseEventKind::ScrollUp => {
                self.active_tab_mut().viewport.scroll_up(3);
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                self.active_tab_mut().viewport.scroll_down(3);
                self.dirty = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.active_tab().editor.has_selection() {
                    self.active_tab_mut().editor.clear_selection();
                    self.dirty = true;
                }
                if let Some(action) = self
                    .hit_regions
                    .iter()
                    .find(|region| region.contains(column, row))
                    .map(|region| region.action)
                {
                    if self.active_tab().viewport.selection.is_some() {
                        self.active_tab_mut().viewport.clear_selection();
                        self.dirty = true;
                    }
                    return match action {
                        HitAction::SwitchTab(idx) => {
                            if idx < self.tabs.len() && idx != self.active_tab_index {
                                Some(AppAction::SwitchTab(idx))
                            } else {
                                None
                            }
                        }
                        HitAction::CloseOrResetTab(idx) => {
                            self.dirty = true;
                            Some(AppAction::CloseOrResetTab(idx))
                        }
                        HitAction::NewTab => {
                            self.dirty = true;
                            Some(AppAction::NewTab)
                        }
                    };
                }
                let (vx, vy, vw, vh) = self.viewport_bounds;
                if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                    let tab = self.active_tab_mut();
                    let offset = tab.viewport.effective_offset();
                    let line = offset + (row.saturating_sub(vy) as usize);
                    let col = column.saturating_sub(vx) as usize;
                    tab.viewport.start_selection(line, col);
                    self.dirty = true;
                } else if self.active_tab().viewport.selection.is_some() {
                    self.active_tab_mut().viewport.clear_selection();
                    self.dirty = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let (vx, vy, vw, vh) = self.viewport_bounds;
                if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                    let tab = self.active_tab_mut();
                    let offset = tab.viewport.effective_offset();
                    let line = offset + (row.saturating_sub(vy) as usize);
                    let col = column.saturating_sub(vx) as usize;
                    tab.viewport.update_selection(line, col);
                    self.dirty = true;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if matches!(self.active_tab().viewport.selection, Some((s, e)) if s == e) {
                    self.active_tab_mut().viewport.clear_selection();
                    self.dirty = true;
                    let (vx, vy, vw, vh) = self.viewport_bounds;
                    if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                        let tab = self.active_tab_mut();
                        let offset = tab.viewport.effective_offset();
                        let line = offset + (row.saturating_sub(vy) as usize);
                        let _ = tab.cache.toggle_collapsible_at_line(line);
                    }
                }
            }
            _ => {}
        }
        None
    }
}
