use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::app::{App, AppAction};
use crate::ui::editor::{ExtmarkKind, TriggerKind};
use crate::ui::modal::{CommandPaletteModal, Modal};

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Option<AppAction> {
        if key.kind != KeyEventKind::Press {
            return None;
        }

        let is_ctrl_c =
            key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c');
        if !is_ctrl_c {
            self.last_ctrl_c_at = None;
        }

        if !self.modals.is_empty() {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
                self.modals.pop();
                return Some(AppAction::NewTab);
            }
            let action = self.modals.handle_key_event(key);
            self.dirty = true;
            if let Some(res) = self.dispatch_modal_action(action) {
                return res;
            }
            return None;
        }

        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if let Some(digit) = c.to_digit(10) {
                    if (1..=9).contains(&digit) {
                        let idx = (digit - 1) as usize;
                        if idx < self.tabs.len() {
                            self.switch_tab(idx);
                            return Some(AppAction::SwitchTab(idx));
                        }
                    }
                }
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.modals
                .push(Modal::CommandPalette(CommandPaletteModal::new()));
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w') {
            return Some(AppAction::CloseTab(self.active_tab_index));
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            let sel = self
                .viewport
                .selected_text(&self.cache)
                .or_else(|| self.editor.selected_text());
            if let Some(selected) = sel {
                if !selected.is_empty() {
                    let _ = self.clipboard.set_text(&selected);
                    self.viewport.clear_selection();
                    self.editor.clear_selection();
                    self.last_ctrl_c_at = None;
                    self.status_text = String::from("Selection copied to clipboard");
                    self.dirty = true;
                    return None;
                }
            }

            if !self.editor.is_empty() {
                self.editor.record_undo_snapshot();
                self.editor.clear();
                self.popover.close();
                self.draft_snapshot = None;
                self.last_ctrl_c_at = None;
                self.status_text = String::from("Input cleared");
                self.dirty = true;
                return None;
            }

            if self.is_turn_active() {
                let _ = self.session_manager.interrupt_active();
                self.queued_prompts.clear();
                self.cache.consolidate_active_turn_with_meta(None, None);
                self.status_text = String::from("Interrupted by user");
                self.waiting_for_response = false;
                self.queued_prompts.clear();
                self.last_ctrl_c_at = None;
                self.dirty = true;
                return Some(AppAction::Interrupt);
            }

            if let Some(last) = self.last_ctrl_c_at {
                if last.elapsed() <= std::time::Duration::from_millis(1500) {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }
            }

            self.last_ctrl_c_at = Some(std::time::Instant::now());
            self.status_text = String::from("Press Ctrl+C again or Ctrl+Q to quit");
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Some(AppAction::Quit);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
            if let Some(text) = self.clipboard.get_text() {
                self.editor.insert_paste(&text);
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
                return None;
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
            self.viewport.clear_selection();
            self.editor.select_all();
            self.popover.close();
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
            if self.editor.undo() {
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            if self.editor.delete_to_line_start() {
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            return None;
        }

        if self.popover.is_visible() {
            match key.code {
                KeyCode::Up => {
                    self.popover.select_prev();
                    self.dirty = true;
                    return None;
                }
                KeyCode::Down => {
                    self.popover.select_next();
                    self.dirty = true;
                    return None;
                }
                KeyCode::Esc => {
                    self.popover.close();
                    self.dirty = true;
                    return None;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(item) = self.popover.selected_item().cloned() {
                        let start = self.popover.trigger_start_char;
                        let end = self.editor.cursor_char();
                        let kind = match item.kind {
                            TriggerKind::File => ExtmarkKind::FileMention {
                                path: item.label.clone(),
                            },
                            TriggerKind::Command => ExtmarkKind::SlashCommand {
                                cmd: item.insert_text.trim().to_string(),
                            },
                        };
                        self.editor
                            .replace_range_with_extmark(start, end, kind, &item.insert_text);
                        self.draft_snapshot = None;
                    }
                    self.popover.close();
                    self.dirty = true;
                    return None;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Esc {
            if self.editor.has_selection() {
                self.editor.clear_selection();
                self.dirty = true;
                return None;
            }
            if self.viewport.selection.is_some() {
                self.viewport.clear_selection();
                self.dirty = true;
                return None;
            }
            if self.popover.is_visible() {
                self.popover.close();
                self.dirty = true;
                return None;
            }
            if !self.status_text.is_empty() && self.status_text != "Ready" {
                self.status_text = String::from("Ready");
                self.dirty = true;
                return None;
            }
            return None;
        }

        if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(AppAction::NewTab);
        }

        if key.code == KeyCode::Tab {
            self.cache.toggle_latest_thought();
            self.dirty = true;
            return None;
        }

        if key.code == KeyCode::BackTab
            || (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.cache.toggle_latest_tool();
            self.dirty = true;
            return None;
        }

        let is_multiline_newline = (key.code == KeyCode::Enter
            && (key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT)))
            || (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL));

        if is_multiline_newline {
            self.editor.insert_newline();
            self.draft_snapshot = None;
            self.dirty = true;
            self.update_autocomplete();
            return None;
        }

        match key.code {
            KeyCode::Enter => {
                let prompt = self.editor.take_submitted_expanded();
                if let Some(p) = prompt {
                    self.draft_snapshot = None;
                    let trimmed = p.trim();
                    if let Some(cmd_action) = self.execute_local_command(trimmed) {
                        return cmd_action;
                    }

                    self.history.record_prompt(&p);
                    if self.is_turn_active() {
                        self.queued_prompts.push_back(p);
                        self.status_text =
                            format!("Queued ({} pending)", self.queued_prompts.len());
                        self.dirty = true;
                        return None;
                    }
                    let active_id = self.active_session.clone();
                    self.session_manager.set_session_prompt(&active_id, &p);
                    self.sync_session_to_history(&active_id);
                    self.history.touch_session_timestamp(active_id.as_str());
                    self.cache.push_user_message(&p);
                    self.viewport.scroll_to_bottom();
                    self.waiting_for_response = true;
                    self.turn_started_at = Some(std::time::Instant::now());
                    self.dirty = true;
                    self.status_text = String::from("Waiting for response...");
                    return Some(AppAction::SubmitPrompt(p));
                }
            }
            KeyCode::Backspace => {
                self.editor.backspace();
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Delete => {
                self.editor.delete_forward();
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Left => {
                self.editor.move_left();
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Right => {
                self.editor.move_right();
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Home => {
                self.editor.move_home();
                self.dirty = true;
            }
            KeyCode::End => {
                self.editor.move_end();
                self.dirty = true;
            }
            KeyCode::Up => {
                if self.editor.move_up() {
                    self.dirty = true;
                } else {
                    if self.draft_snapshot.is_none() {
                        self.draft_snapshot = Some(self.editor.snapshot());
                    }
                    let draft = self.editor.text();
                    if let Some(prev) = self.history.prev_history(&draft) {
                        self.editor.clear();
                        self.editor.insert_str(&prev);
                        self.dirty = true;
                    } else {
                        self.viewport.scroll_up(1);
                        self.dirty = true;
                    }
                }
            }
            KeyCode::Down => {
                if self.editor.move_down() {
                    self.dirty = true;
                } else if let Some(next) = self.history.next_history() {
                    if self.history.is_navigating() {
                        self.editor.clear();
                        self.editor.insert_str(&next);
                        self.dirty = true;
                    } else if let Some(draft) = self.draft_snapshot.take() {
                        self.editor.restore_snapshot(draft);
                        self.dirty = true;
                    } else {
                        self.editor.clear();
                        self.editor.insert_str(&next);
                        self.dirty = true;
                    }
                } else if let Some(draft) = self.draft_snapshot.take() {
                    self.editor.restore_snapshot(draft);
                    self.dirty = true;
                } else {
                    self.viewport.scroll_down(1);
                    self.dirty = true;
                }
            }
            KeyCode::PageUp => {
                self.viewport.scroll_up(10);
                self.dirty = true;
            }
            KeyCode::PageDown => {
                self.viewport.scroll_down(10);
                self.dirty = true;
            }
            KeyCode::Char(c) => {
                if self.viewport.selection.is_some() {
                    self.viewport.clear_selection();
                }
                self.editor.insert_char(c);
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            _ => {}
        }
        None
    }
}
