mod keyboard;
mod mouse;

use crossterm::event::Event;

use crate::protocol::SessionId;
use crate::ui::app::{App, AppAction};
use crate::ui::autocomplete::AutocompletePopover;
use crate::ui::clipboard::SavedMediaImage;
use crate::ui::editor::{extract_pasted_image_path, TriggerKind};

impl App {
    pub fn resolve_pasted_text(text: String) -> (Option<SavedMediaImage>, Option<String>) {
        if let Some((path, mime)) = extract_pasted_image_path(&text) {
            (
                Some(SavedMediaImage {
                    path,
                    mime_type: mime,
                }),
                None,
            )
        } else {
            (None, Some(text))
        }
    }

    pub fn apply_paste_outcome(
        &mut self,
        session_id: &SessionId,
        outcome: Result<(Option<SavedMediaImage>, Option<String>), String>,
    ) {
        let Some(tab_idx) = self.tabs.iter().position(|t| &t.session_id == session_id) else {
            return;
        };
        let is_active = tab_idx == self.active_tab_index;
        let tab = &mut self.tabs[tab_idx];
        match outcome {
            Ok((Some(saved), _)) => {
                if tab.viewport.selection.is_some() {
                    tab.viewport.clear_selection();
                }
                tab.editor.insert_media_attachment(
                    saved.mime_type,
                    saved.path.to_string_lossy().into_owned(),
                );
                let label = saved
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image");
                tab.status_text = format!("Attached image: {}", label);
                tab.draft_snapshot = None;
            }
            Ok((None, Some(text))) => {
                if tab.viewport.selection.is_some() {
                    tab.viewport.clear_selection();
                }
                tab.editor.insert_paste(&text);
                tab.draft_snapshot = None;
            }
            Ok((None, None)) => return,
            Err(err) => {
                tab.status_text = format!("Clipboard image error: {}", err);
            }
        }
        self.dirty = true;
        if is_active {
            self.update_autocomplete();
        }
    }

    pub fn handle_clipboard_paste_trigger(&mut self) -> Option<AppAction> {
        if !self.clipboard.is_isolated() {
            return Some(AppAction::PasteClipboard);
        }
        let sid = self.active_session_id().clone();
        match self.clipboard.get_image() {
            Ok(Some(saved)) => {
                self.apply_paste_outcome(&sid, Ok((Some(saved), None)));
                return None;
            }
            Err(err) => {
                self.apply_paste_outcome(&sid, Err(err));
                return None;
            }
            Ok(None) => {}
        }
        if let Some(text) = self.clipboard.get_text() {
            self.apply_paste_outcome(&sid, Ok(Self::resolve_pasted_text(text)));
        }
        None
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Option<AppAction> {
        match event {
            Event::Key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Paste(text) => {
                if text.is_empty() {
                    return self.handle_clipboard_paste_trigger();
                }
                if !self.clipboard.is_isolated() {
                    return Some(AppAction::InspectPastedText(text));
                }
                let sid = self.active_session_id().clone();
                self.apply_paste_outcome(&sid, Ok(Self::resolve_pasted_text(text)));
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
        if let Some((kind, start_char, query)) = self.active_tab().editor.active_trigger() {
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
