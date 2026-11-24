use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::app::{App, AppAction};
use crate::ui::editor::{ExtmarkKind, TriggerKind};
use crate::ui::modal::{CommandsModal, Modal};

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
        if key.code != KeyCode::Esc {
            self.active_tab_mut().esc_armed_at = None;
        }

        if !self.modals.is_empty() {
            self.active_tab_mut().esc_armed_at = None;
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
                            return Some(AppAction::SwitchTab(idx));
                        }
                    }
                }
            }
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('b') {
            self.sidebar_visible = !self.sidebar_visible;
            self.active_tab_mut().status_text = if self.sidebar_visible {
                String::from("Context sidebar shown")
            } else {
                String::from("Context sidebar hidden")
            };
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            self.modals.push(Modal::Commands(CommandsModal::new()));
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('w') {
            return Some(AppAction::CloseTab(self.active_tab_index));
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            let sel = {
                let tab = self.active_tab();
                tab.viewport
                    .selected_text(&tab.cache)
                    .or_else(|| tab.editor.selected_text())
            };
            if let Some(selected) = sel {
                if !selected.is_empty() {
                    let tab = self.active_tab_mut();
                    tab.viewport.clear_selection();
                    tab.editor.clear_selection();
                    tab.status_text = String::from("Selection copied to clipboard");
                    self.last_ctrl_c_at = None;
                    self.dirty = true;
                    if self.clipboard.is_isolated() {
                        let _ = self.clipboard.set_text(&selected);
                        return None;
                    }
                    return Some(AppAction::CopyClipboard(selected));
                }
            }

            if !self.active_tab().editor.is_empty() {
                let tab = self.active_tab_mut();
                tab.editor.record_undo_snapshot();
                tab.editor.clear();
                tab.draft_snapshot = None;
                tab.status_text = String::from("Input cleared");
                self.popover.close();
                self.last_ctrl_c_at = None;
                self.dirty = true;
                return None;
            }

            if let Some(last) = self.last_ctrl_c_at {
                if last.elapsed() <= std::time::Duration::from_millis(1500) {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }
            }

            self.last_ctrl_c_at = Some(std::time::Instant::now());
            self.active_tab_mut().status_text =
                String::from("Press Ctrl+C again or Ctrl+Q to quit");
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            self.should_quit = true;
            return Some(AppAction::Quit);
        }

        let is_paste_shortcut = (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT))
            && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'));
        if is_paste_shortcut {
            return self.handle_clipboard_paste_trigger();
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('a') {
            let tab = self.active_tab_mut();
            tab.viewport.clear_selection();
            tab.editor.select_all();
            self.popover.close();
            self.dirty = true;
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('z') {
            let undone = {
                let tab = self.active_tab_mut();
                if tab.editor.undo() {
                    tab.draft_snapshot = None;
                    true
                } else {
                    false
                }
            };
            if undone {
                self.dirty = true;
                self.update_autocomplete();
            }
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            let deleted = {
                let tab = self.active_tab_mut();
                if tab.editor.delete_to_line_start() {
                    tab.draft_snapshot = None;
                    true
                } else {
                    false
                }
            };
            if deleted {
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
                    self.active_tab_mut().esc_armed_at = None;
                    self.dirty = true;
                    return None;
                }
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(item) = self.popover.selected_item().cloned() {
                        let start = self.popover.trigger_start_char;
                        let end = self.active_tab().editor.cursor_char();
                        let kind = match item.kind {
                            TriggerKind::File => ExtmarkKind::FileMention {
                                path: item.label.clone(),
                            },
                            TriggerKind::Command => ExtmarkKind::SlashCommand {
                                cmd: item.insert_text.trim().to_string(),
                            },
                        };
                        let tab = self.active_tab_mut();
                        tab.editor
                            .replace_range_with_extmark(start, end, kind, &item.insert_text);
                        tab.draft_snapshot = None;
                    }
                    self.popover.close();
                    self.dirty = true;
                    return None;
                }
                _ => {}
            }
        }

        if key.code == KeyCode::Esc {
            if self.active_tab().editor.has_selection() {
                let tab = self.active_tab_mut();
                tab.editor.clear_selection();
                tab.esc_armed_at = None;
                self.dirty = true;
                return None;
            }
            if self.active_tab().viewport.selection.is_some() {
                let tab = self.active_tab_mut();
                tab.viewport.clear_selection();
                tab.esc_armed_at = None;
                self.dirty = true;
                return None;
            }
            if self.is_turn_active() {
                if self.active_tab().is_interrupt_armed() {
                    {
                        let tab = self.active_tab_mut();
                        tab.cache.consolidate_active_turn_with_meta(None, None);
                        tab.cache.push_interrupted();
                        tab.transition_turn(crate::ui::tab::TurnEvent::InterruptRequested);
                        tab.status_text = String::from("Interrupted by user");
                    }
                    let active_idx = self.active_tab_index;
                    self.persist_completed_turn(active_idx);
                    self.dirty = true;
                    return Some(AppAction::Interrupt);
                }
                self.active_tab_mut().esc_armed_at = Some(std::time::Instant::now());
                self.dirty = true;
                return None;
            }
            if !self.active_tab().status_text.is_empty() && self.active_tab().status_text != "Ready"
            {
                self.active_tab_mut().status_text = String::from("Ready");
                self.dirty = true;
                return None;
            }
            return None;
        }

        if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(AppAction::NewTab);
        }

        if key.code == KeyCode::Tab {
            self.active_tab_mut().cache.toggle_latest_thought();
            self.dirty = true;
            return None;
        }

        if key.code == KeyCode::BackTab
            || (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.active_tab_mut().cache.toggle_latest_tool();
            self.dirty = true;
            return None;
        }

        let is_multiline_newline = (key.code == KeyCode::Enter
            && (key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT)))
            || (key.code == KeyCode::Char('j') && key.modifiers.contains(KeyModifiers::CONTROL));

        if is_multiline_newline {
            let tab = self.active_tab_mut();
            tab.editor.insert_newline();
            tab.draft_snapshot = None;
            self.dirty = true;
            self.update_autocomplete();
            return None;
        }

        match key.code {
            KeyCode::Enter => {
                let payload = self.active_tab_mut().editor.take_submitted_payload();
                if let Some(p) = payload {
                    self.active_tab_mut().draft_snapshot = None;
                    if p.media.is_empty() {
                        let trimmed = p.text.trim();
                        if let Some(cmd_action) = self.execute_local_command(trimmed) {
                            return cmd_action;
                        }
                    }

                    let chat_display = p.display_for_chat();
                    let history_entry = if !p.text.trim().is_empty() {
                        p.text.as_str()
                    } else {
                        chat_display.as_str()
                    };
                    self.history.record_prompt(history_entry);
                    if self.is_turn_active() {
                        let tab = self.active_tab_mut();
                        tab.queued_prompts.push_back(p);
                        tab.status_text = format!("Queued ({} pending)", tab.queued_prompts.len());
                        self.dirty = true;
                        return None;
                    }
                    let active_id = self.active_session_id().clone();
                    self.session_manager
                        .set_session_prompt(&active_id, &p.summary_text());
                    self.sync_session_to_history(&active_id);
                    self.history.touch_session_timestamp(active_id.as_str());
                    let tab = self.active_tab_mut();
                    tab.cache.push_user_message(&chat_display);
                    tab.viewport.scroll_to_bottom();
                    tab.transition_turn(crate::ui::tab::TurnEvent::Submit);
                    tab.status_text = String::from("Waiting for response...");
                    self.dirty = true;
                    return Some(AppAction::SubmitPrompt(p));
                }
            }
            KeyCode::Backspace => {
                let tab = self.active_tab_mut();
                tab.editor.backspace();
                tab.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Delete => {
                let tab = self.active_tab_mut();
                tab.editor.delete_forward();
                tab.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Left => {
                self.active_tab_mut().editor.move_left();
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Right => {
                self.active_tab_mut().editor.move_right();
                self.dirty = true;
                self.update_autocomplete();
            }
            KeyCode::Home => {
                self.active_tab_mut().editor.move_home();
                self.dirty = true;
            }
            KeyCode::End => {
                self.active_tab_mut().editor.move_end();
                self.dirty = true;
            }
            KeyCode::Up => {
                if self.active_tab_mut().editor.move_up() {
                    self.dirty = true;
                } else {
                    let draft = {
                        let tab = self.active_tab_mut();
                        if tab.draft_snapshot.is_none() {
                            tab.draft_snapshot = Some(tab.editor.snapshot());
                        }
                        tab.editor.text()
                    };
                    if let Some(prev) = self.history.prev_history(&draft) {
                        let tab = self.active_tab_mut();
                        tab.editor.clear();
                        tab.editor.insert_str(&prev);
                        self.dirty = true;
                    } else {
                        self.active_tab_mut().viewport.scroll_up(1);
                        self.dirty = true;
                    }
                }
            }
            KeyCode::Down => {
                if self.active_tab_mut().editor.move_down() {
                    self.dirty = true;
                } else if let Some(next) = self.history.next_history() {
                    let is_nav = self.history.is_navigating();
                    let tab = self.active_tab_mut();
                    if is_nav {
                        tab.editor.clear();
                        tab.editor.insert_str(&next);
                        self.dirty = true;
                    } else if let Some(draft) = tab.draft_snapshot.take() {
                        tab.editor.restore_snapshot(draft);
                        self.dirty = true;
                    } else {
                        tab.editor.clear();
                        tab.editor.insert_str(&next);
                        self.dirty = true;
                    }
                } else {
                    let tab = self.active_tab_mut();
                    if let Some(draft) = tab.draft_snapshot.take() {
                        tab.editor.restore_snapshot(draft);
                        self.dirty = true;
                    } else {
                        tab.viewport.scroll_down(1);
                        self.dirty = true;
                    }
                }
            }
            KeyCode::PageUp => {
                self.active_tab_mut().viewport.scroll_up(10);
                self.dirty = true;
            }
            KeyCode::PageDown => {
                self.active_tab_mut().viewport.scroll_down(10);
                self.dirty = true;
            }
            KeyCode::Char(c) => {
                let tab = self.active_tab_mut();
                if tab.viewport.selection.is_some() {
                    tab.viewport.clear_selection();
                }
                tab.editor.insert_char(c);
                tab.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            _ => {}
        }
        None
    }
}
