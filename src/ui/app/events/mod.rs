mod keyboard;
mod mouse;

use crossterm::event::Event;

use crate::ui::app::{App, AppAction};
use crate::ui::autocomplete::AutocompletePopover;
use crate::ui::editor::TriggerKind;

impl App {
    pub fn handle_terminal_event(&mut self, event: Event) -> Option<AppAction> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Paste(text) => {
                if self.viewport.selection.is_some() {
                    self.viewport.clear_selection();
                }
                self.editor.insert_paste(&text);
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
                None
            }
            Event::Resize(_, _) => {
                self.dirty = true;
                None
            }
            _ => None,
        }
    }

    pub fn update_autocomplete(&mut self) {
        if let Some((kind, start_char, query)) = self.editor.active_trigger() {
            match kind {
                TriggerKind::Command => {
                    let items = AutocompletePopover::filter_commands(&query);
                    self.popover.show(kind, start_char, &query, items);
                }
                TriggerKind::File => {
                    let items = self.search_worker.search(&query, 20);
                    self.popover.show(kind, start_char, &query, items);
                }
            }
        } else {
            self.popover.close();
        }
    }
}
