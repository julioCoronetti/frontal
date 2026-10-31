use crossterm::event::{
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::protocol::{EngineEvent, SessionEnvelope, SessionId, StepState, StreamEvent};
use crate::session::{SessionManager, SessionStatus};
use crate::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use crate::ui::cache::{BlockKind, RenderCache};
use crate::ui::clipboard::SystemClipboard;
use crate::ui::editor::{EditorSnapshot, ExtmarkKind, PromptEditor, TriggerKind};
use crate::ui::history::PromptHistory;
use crate::ui::input_stub::MinimalInputStub;
use crate::ui::modal::{
    CommandPaletteModal, DiffModal, HotRespawnModal, Modal, ModalAction, ModalStack,
    ModelEffortModal,
};
use crate::ui::notify::emit_sensory_notification;
use crate::ui::tab::TabState;
use crate::ui::theme::{ACCENT_PRIMARY, BORDER_SUBTLE, TAB_ACTIVE_DOT, TEXT_MUTED, TEXT_PRIMARY};
use crate::ui::viewport::ConversationViewport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppAction {
    SubmitPrompt(String),
    Interrupt,
    Quit,
    NewTab,
    SwitchTab(usize),
    CloseTab(usize),
    RespawnSession {
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    },
}

pub struct App {
    pub session_manager: SessionManager,
    pub active_session: SessionId,
    pub cache: RenderCache,
    pub viewport: ConversationViewport,
    pub input: MinimalInputStub,
    pub editor: PromptEditor,
    pub popover: AutocompletePopover,
    pub search_worker: FileSearchWorker,
    pub history: PromptHistory,
    pub draft_snapshot: Option<EditorSnapshot>,
    pub dirty: bool,
    pub should_quit: bool,
    pub status_text: String,
    pub spinner_tick: u64,
    pub waiting_for_response: bool,
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
    pub modals: ModalStack,
    pub clipboard: SystemClipboard,
    pub cwd_display: String,
    pub available_models: Vec<(String, String)>,
    pub viewport_bounds: (u16, u16, u16, u16),
    pub turn_started_at: Option<std::time::Instant>,
}

impl App {
    pub fn new(session_manager: SessionManager, active_session: SessionId) -> Self {
        let history = if let Ok(home) = std::env::var("HOME") {
            let path = std::path::PathBuf::from(home)
                .join(".frontal")
                .join("history.db");
            PromptHistory::open(&path).unwrap_or_else(|_| PromptHistory::open_in_memory().unwrap())
        } else {
            PromptHistory::open_in_memory().unwrap()
        };

        let search_worker = FileSearchWorker::new();
        if let Ok(cwd) = std::env::current_dir() {
            search_worker.start_background_scan(cwd, 500);
        }

        let initial_tab = TabState::new(active_session.clone(), String::from("Session 1"));
        let cwd_path = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let cwd_display = if let Ok(home) = std::env::var("HOME") {
            let home_path = std::path::PathBuf::from(&home);
            if let Ok(rel) = cwd_path.strip_prefix(&home_path) {
                format!("~/{}", rel.display())
            } else {
                cwd_path.display().to_string()
            }
        } else {
            cwd_path.display().to_string()
        };
        let available_models = crate::transport::EngineConfig::default_fallback_models();

        Self {
            session_manager,
            active_session,
            cache: RenderCache::new(),
            viewport: ConversationViewport::new(),
            input: MinimalInputStub::new(),
            editor: PromptEditor::new(),
            popover: AutocompletePopover::new(),
            search_worker,
            history,
            draft_snapshot: None,
            dirty: true,
            should_quit: false,
            status_text: String::from("Ready"),
            spinner_tick: 0,
            waiting_for_response: false,
            tabs: vec![initial_tab],
            active_tab_index: 0,
            modals: ModalStack::new(),
            clipboard: SystemClipboard::new(),
            cwd_display,
            available_models,
            viewport_bounds: (0, 0, 80, 20),
            turn_started_at: None,
        }
    }

    pub fn new_isolated(session_manager: SessionManager, active_session: SessionId) -> Self {
        let history = PromptHistory::open_in_memory().unwrap();
        let search_worker = FileSearchWorker::new();
        let initial_tab = TabState::new(active_session.clone(), String::from("Session 1"));
        let cwd_display = String::from("~/test");
        let available_models = crate::transport::EngineConfig::default_fallback_models();

        Self {
            session_manager,
            active_session,
            cache: RenderCache::new(),
            viewport: ConversationViewport::new(),
            input: MinimalInputStub::new(),
            editor: PromptEditor::new(),
            popover: AutocompletePopover::new(),
            search_worker,
            history,
            draft_snapshot: None,
            dirty: true,
            should_quit: false,
            status_text: String::from("Ready"),
            spinner_tick: 0,
            waiting_for_response: false,
            tabs: vec![initial_tab],
            active_tab_index: 0,
            modals: ModalStack::new(),
            clipboard: SystemClipboard::new_isolated(),
            cwd_display,
            available_models,
            viewport_bounds: (0, 0, 80, 20),
            turn_started_at: None,
        }
    }

    pub fn set_available_models(&mut self, models: Vec<(String, String)>) {
        self.available_models = models;
    }

    pub fn active_model_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.model.clone())
            .unwrap_or_else(|| "gemini-4-argon-high".to_string())
    }

    pub fn active_mode_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.mode.clone())
            .unwrap_or_else(|| "auto".to_string())
    }

    pub fn active_effort_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.effort.clone())
            .unwrap_or_else(|| "high".to_string())
    }

    pub fn active_conversation_id(&self) -> Option<String> {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.conversation_id.clone())
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab(&self) -> Option<&TabState> {
        self.tabs.get(self.active_tab_index)
    }

    pub fn switch_tab(&mut self, new_index: usize) -> bool {
        if new_index >= self.tabs.len() || new_index == self.active_tab_index {
            return false;
        }
        self.popover.close();
        self.save_active_tab_state();
        self.active_tab_index = new_index;
        self.load_active_tab_state();
        self.dirty = true;
        true
    }

    pub fn save_active_tab_state(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            std::mem::swap(&mut self.cache, &mut tab.cache);
            std::mem::swap(&mut self.viewport, &mut tab.viewport);
            std::mem::swap(&mut self.editor, &mut tab.editor);
            std::mem::swap(&mut self.input, &mut tab.input);
            tab.waiting_for_response = self.waiting_for_response;
            tab.turn_started_at = self.turn_started_at;
            std::mem::swap(&mut self.status_text, &mut tab.status_text);
            std::mem::swap(&mut self.draft_snapshot, &mut tab.draft_snapshot);
            tab.session_id = self.active_session.clone();
        }
    }

    pub fn load_active_tab_state(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            std::mem::swap(&mut self.cache, &mut tab.cache);
            std::mem::swap(&mut self.viewport, &mut tab.viewport);
            std::mem::swap(&mut self.editor, &mut tab.editor);
            std::mem::swap(&mut self.input, &mut tab.input);
            self.waiting_for_response = tab.waiting_for_response;
            self.turn_started_at = tab.turn_started_at;
            std::mem::swap(&mut self.status_text, &mut tab.status_text);
            std::mem::swap(&mut self.draft_snapshot, &mut tab.draft_snapshot);
            self.active_session = tab.session_id.clone();
            tab.has_unread = false;
            self.session_manager
                .set_active_session(self.active_session.clone());
        }
    }

    pub fn add_tab(&mut self, session_id: SessionId, title: Option<String>) -> usize {
        self.popover.close();
        self.save_active_tab_state();
        let title = title.unwrap_or_else(|| format!("Session {}", self.tabs.len() + 1));
        let new_tab = TabState::new(session_id, title);
        self.tabs.push(new_tab);
        self.active_tab_index = self.tabs.len() - 1;
        self.load_active_tab_state();
        self.dirty = true;
        self.active_tab_index
    }

    pub async fn close_tab(&mut self, index: usize) -> Option<SessionId> {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return None;
        }
        if index == self.active_tab_index {
            let removed_tab = self.tabs.remove(index);
            let _ = self
                .session_manager
                .shutdown_session(
                    &removed_tab.session_id,
                    std::time::Duration::from_millis(500),
                )
                .await;
            self.cache = RenderCache::new();
            self.viewport = ConversationViewport::new();
            self.editor = PromptEditor::new();
            self.input = MinimalInputStub::new();
            self.draft_snapshot = None;
            self.waiting_for_response = false;
            self.turn_started_at = None;
            self.status_text = String::from("Ready");
            self.popover.close();
            self.active_tab_index = self.active_tab_index.min(self.tabs.len() - 1);
            self.load_active_tab_state();
            self.dirty = true;
            Some(removed_tab.session_id)
        } else {
            let removed_tab = self.tabs.remove(index);
            let _ = self
                .session_manager
                .shutdown_session(
                    &removed_tab.session_id,
                    std::time::Duration::from_millis(500),
                )
                .await;
            if index < self.active_tab_index {
                self.active_tab_index -= 1;
            }
            self.popover.close();
            self.dirty = true;
            Some(removed_tab.session_id)
        }
    }

    pub async fn respawn_active_session(
        &mut self,
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> Result<SessionId, crate::transport::EngineError> {
        let old_id = self.active_session.clone();
        let was_replay = self.session_manager.is_replay_session(&old_id);
        let (prev_conv_id, prev_model, prev_effort, prev_mode) = {
            let meta = self.session_manager.get_metadata(&old_id);
            (
                meta.and_then(|m| m.conversation_id.clone()),
                meta.and_then(|m| m.model.clone()),
                meta.and_then(|m| m.effort.clone()),
                meta.and_then(|m| m.mode.clone()),
            )
        };
        let _ = self
            .session_manager
            .shutdown_session(&old_id, std::time::Duration::from_millis(500))
            .await;
        let final_conv_id = conversation_id.or(prev_conv_id);
        let final_model = model.or(prev_model);
        let final_effort = effort.or(prev_effort);
        let final_mode = mode.or(prev_mode);

        let new_id = if was_replay {
            let id = SessionId::new();
            self.session_manager.register_mock_session(id.clone());
            self.session_manager.set_session_metadata(
                &id,
                final_conv_id,
                final_model,
                final_effort,
                final_mode,
            );
            id
        } else {
            let mut config = crate::transport::EngineConfig::new();
            config.conversation_id = final_conv_id;
            config.model = final_model;
            config.effort = final_effort;
            config.mode = final_mode;
            self.session_manager
                .create_subprocess_session(config)
                .await?
        };
        self.active_session = new_id.clone();
        self.session_manager.set_active_session(new_id.clone());
        if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
            tab.session_id = new_id.clone();
        }
        self.status_text = format!("Respawned session: {}", new_id);
        self.dirty = true;
        Ok(new_id)
    }

    pub fn load_git_diff() -> String {
        if let Ok(out) = std::process::Command::new("git").args(["diff"]).output() {
            if !out.stdout.is_empty() {
                return String::from_utf8_lossy(&out.stdout).to_string();
            }
        }
        if let Ok(out) = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .output()
        {
            if !out.stdout.is_empty() {
                return String::from_utf8_lossy(&out.stdout).to_string();
            }
        }
        String::from("diff --git (working tree clean)\nNo unstaged or uncommitted changes found.\n")
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Option<AppAction> {
        match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return None;
                }

                if !self.modals.is_empty() {
                    let action = self.modals.handle_key_event(key);
                    self.dirty = true;
                    match action {
                        ModalAction::Close => return None,
                        ModalAction::SelectModelEffort { model, effort } => {
                            self.modals.pop();
                            self.status_text = format!("Model: {} ({})", model, effort);
                            return Some(AppAction::RespawnSession {
                                conversation_id: None,
                                model: Some(model),
                                effort: Some(effort),
                                mode: None,
                            });
                        }
                        ModalAction::Respawn {
                            conversation_id,
                            model,
                            effort,
                            mode,
                        } => {
                            self.modals.pop();
                            return Some(AppAction::RespawnSession {
                                conversation_id,
                                model,
                                effort,
                                mode,
                            });
                        }
                        ModalAction::ExecuteCommand(cmd) => {
                            self.modals.pop();
                            match cmd.as_str() {
                                "new_tab" => return Some(AppAction::NewTab),
                                "close_tab" => {
                                    return Some(AppAction::CloseTab(self.active_tab_index))
                                }
                                "select_model" => {
                                    self.modals.push(Modal::ModelEffort(
                                        ModelEffortModal::with_models(
                                            self.available_models.clone(),
                                        ),
                                    ));
                                    return None;
                                }
                                "hot_respawn" => {
                                    let conv_id = self
                                        .session_manager
                                        .get_metadata(&self.active_session)
                                        .and_then(|m| m.conversation_id.clone());
                                    let model = self
                                        .session_manager
                                        .get_metadata(&self.active_session)
                                        .and_then(|m| m.model.clone());
                                    let effort = self
                                        .session_manager
                                        .get_metadata(&self.active_session)
                                        .and_then(|m| m.effort.clone());
                                    let mode = self
                                        .session_manager
                                        .get_metadata(&self.active_session)
                                        .and_then(|m| m.mode.clone());
                                    self.modals.push(Modal::HotRespawn(HotRespawnModal::new(
                                        conv_id, model, effort, mode,
                                    )));
                                    return None;
                                }
                                "view_diff" => {
                                    self.modals
                                        .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
                                    return None;
                                }
                                "clear" => {
                                    self.cache.clear();
                                    self.status_text = String::from("Cleared");
                                    return None;
                                }
                                "interrupt" => {
                                    let _ = self.session_manager.interrupt_active();
                                    self.status_text = String::from("Interrupted");
                                    self.waiting_for_response = false;
                                    return Some(AppAction::Interrupt);
                                }
                                "quit" => {
                                    self.should_quit = true;
                                    return Some(AppAction::Quit);
                                }
                                _ => return None,
                            }
                        }
                        ModalAction::None => return None,
                    }
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
                    let is_running = self
                        .session_manager
                        .get_metadata(&self.active_session)
                        .map(|m| m.status == SessionStatus::RunningTurn)
                        .unwrap_or(false);
                    if is_running {
                        let _ = self.session_manager.interrupt_active();
                        self.status_text = String::from("Interrupted by user");
                        self.waiting_for_response = false;
                        self.dirty = true;
                        return Some(AppAction::Interrupt);
                    } else {
                        self.should_quit = true;
                        return Some(AppAction::Quit);
                    }
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
                    if let Some(text) = self.clipboard.get_text() {
                        self.editor.insert_paste(&text);
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                        return None;
                    }
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
                                self.editor.replace_range_with_extmark(
                                    start,
                                    end,
                                    kind,
                                    &item.insert_text,
                                );
                                self.input.clear();
                                self.input.insert_str(&self.editor.text());
                                self.draft_snapshot = None;
                            }
                            self.popover.close();
                            self.dirty = true;
                            return None;
                        }
                        _ => {}
                    }
                }

                if key.code == KeyCode::Esc && self.editor.is_empty() && self.input.is_empty() {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
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
                    || (key.code == KeyCode::Char('o')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    self.cache.toggle_latest_tool();
                    self.dirty = true;
                    return None;
                }

                let is_multiline_newline = (key.code == KeyCode::Enter
                    && (key.modifiers.contains(KeyModifiers::SHIFT)
                        || key.modifiers.contains(KeyModifiers::ALT)))
                    || (key.code == KeyCode::Char('j')
                        && key.modifiers.contains(KeyModifiers::CONTROL));

                if is_multiline_newline {
                    self.editor.insert_newline();
                    self.input.clear();
                    self.input.insert_str(&self.editor.text());
                    self.draft_snapshot = None;
                    self.dirty = true;
                    self.update_autocomplete();
                    return None;
                }

                match key.code {
                    KeyCode::Enter => {
                        let prompt = self
                            .editor
                            .take_submitted_expanded()
                            .or_else(|| self.input.take_submitted());
                        if let Some(p) = prompt {
                            self.input.clear();
                            self.draft_snapshot = None;
                            let trimmed = p.trim();
                            if trimmed == "/quit" || trimmed == "/exit" {
                                self.should_quit = true;
                                return Some(AppAction::Quit);
                            } else if trimmed == "/clear" {
                                self.cache.clear();
                                self.status_text = String::from("Cleared");
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/interrupt" {
                                let _ = self.session_manager.interrupt_active();
                                self.status_text = String::from("Interrupted");
                                self.waiting_for_response = false;
                                self.dirty = true;
                                return Some(AppAction::Interrupt);
                            } else if trimmed == "/model" {
                                self.modals.push(Modal::ModelEffort(
                                    ModelEffortModal::with_models(self.available_models.clone()),
                                ));
                                self.dirty = true;
                                return None;
                            } else if trimmed.starts_with("/model ") {
                                let model_name = trimmed.strip_prefix("/model ").unwrap().trim();
                                let mut modal =
                                    ModelEffortModal::with_models(self.available_models.clone());
                                modal.query = model_name.to_string();
                                self.modals.push(Modal::ModelEffort(modal));
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/diff" {
                                self.modals
                                    .push(Modal::Diff(DiffModal::new(&Self::load_git_diff())));
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/respawn" || trimmed == "/resume" {
                                let conv_id = self
                                    .session_manager
                                    .get_metadata(&self.active_session)
                                    .and_then(|m| m.conversation_id.clone());
                                let model = self
                                    .session_manager
                                    .get_metadata(&self.active_session)
                                    .and_then(|m| m.model.clone());
                                let effort = self
                                    .session_manager
                                    .get_metadata(&self.active_session)
                                    .and_then(|m| m.effort.clone());
                                let mode = self
                                    .session_manager
                                    .get_metadata(&self.active_session)
                                    .and_then(|m| m.mode.clone());
                                self.modals.push(Modal::HotRespawn(HotRespawnModal::new(
                                    conv_id, model, effort, mode,
                                )));
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/palette" {
                                self.modals
                                    .push(Modal::CommandPalette(CommandPaletteModal::new()));
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/help" {
                                self.cache.push_user_message("/help");
                                self.cache.push_error(
                                    "Available local commands: /clear, /interrupt, /quit, /exit, /help, /model, /diff, /respawn, /palette",
                                );
                                self.status_text = String::from("Ready");
                                self.dirty = true;
                                return None;
                            } else if trimmed.starts_with('/') {
                                self.cache.push_user_message(trimmed);
                                self.cache.push_error(&format!(
                                    "Command '{}' is not supported locally. Type /help for available commands.",
                                    trimmed
                                ));
                                self.status_text = String::from("Ready");
                                self.dirty = true;
                                return None;
                            }

                            self.history.record_prompt(&p);
                            self.cache.push_user_message(&p);
                            self.viewport.scroll_to_bottom();
                            self.waiting_for_response = true;
                            self.turn_started_at = Some(std::time::Instant::now());
                            self.dirty = true;
                            self.status_text = String::from("Waiting for response...");
                            if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                                tab.waiting_for_response = true;
                                tab.turn_started_at = self.turn_started_at;
                                tab.status_text = self.status_text.clone();
                            }
                            return Some(AppAction::SubmitPrompt(p));
                        }
                    }
                    KeyCode::Backspace => {
                        self.editor.backspace();
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    KeyCode::Delete => {
                        self.editor.delete_forward();
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
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
                                self.input.clear();
                                self.input.insert_str(&prev);
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
                                self.input.clear();
                                self.input.insert_str(&next);
                                self.dirty = true;
                            } else if let Some(draft) = self.draft_snapshot.take() {
                                self.editor.restore_snapshot(draft);
                                self.input.clear();
                                self.input.insert_str(&self.editor.text());
                                self.dirty = true;
                            } else {
                                self.editor.clear();
                                self.editor.insert_str(&next);
                                self.input.clear();
                                self.input.insert_str(&next);
                                self.dirty = true;
                            }
                        } else if let Some(draft) = self.draft_snapshot.take() {
                            self.editor.restore_snapshot(draft);
                            self.input.clear();
                            self.input.insert_str(&self.editor.text());
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
                        self.editor.insert_char(c);
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    _ => {}
                }
            }
            Event::Mouse(MouseEvent {
                kind, column, row, ..
            }) => match kind {
                MouseEventKind::ScrollUp => {
                    self.viewport.scroll_up(3);
                    self.dirty = true;
                }
                MouseEventKind::ScrollDown => {
                    self.viewport.scroll_down(3);
                    self.dirty = true;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let (vx, vy, vw, vh) = self.viewport_bounds;
                    if column >= vx && column < vx + vw && row >= vy && row < vy + vh {
                        let offset = self.viewport.effective_offset();
                        let line = offset + (row.saturating_sub(vy) as usize);
                        let col = column.saturating_sub(vx) as usize;
                        self.viewport.start_selection(line, col);
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
                    if let Some(text) = self.viewport.selected_text(&self.cache) {
                        let _ = self.clipboard.set_text(&text);
                    }
                    self.viewport.clear_selection();
                    self.dirty = true;
                }
                _ => {}
            },
            Event::Paste(text) => {
                self.editor.insert_paste(&text);
                self.input.clear();
                self.input.insert_str(&self.editor.text());
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            Event::Resize(_, _) => {
                self.dirty = true;
            }
            _ => {}
        }
        None
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

    pub fn handle_engine_envelope(&mut self, envelope: SessionEnvelope) {
        if envelope.session_id == self.active_session {
            match envelope.event {
                EngineEvent::Stream(boxed_ev) => match *boxed_ev {
                    StreamEvent::Init(ev) => {
                        if let Some(conv_id) = ev.conversation_id {
                            self.status_text = format!("Conversation: {}", conv_id);
                        }
                        self.dirty = true;
                    }
                    StreamEvent::StepUpdate(update) => {
                        if let Some(data) = update.step_update {
                            if let Some(ref delta) = data.text_delta {
                                self.waiting_for_response = false;
                                self.cache
                                    .append_streaming_delta(BlockKind::Assistant, delta);
                                self.status_text = String::from("Streaming...");
                                self.dirty = true;
                            }
                            if let Some(ref thought) = data.thought {
                                self.waiting_for_response = false;
                                self.cache
                                    .append_streaming_delta(BlockKind::Thought, thought);
                                self.status_text = String::from("Thinking...");
                                self.dirty = true;
                            }
                            if let Some(tool_name) = data.resolved_tool_name() {
                                self.waiting_for_response = false;
                                let state = data.state.unwrap_or(StepState::Active);
                                let args = data
                                    .tool_info
                                    .as_ref()
                                    .and_then(|info| info.parameters_text());
                                let output =
                                    data.tool_info.as_ref().and_then(|info| info.output_text());
                                self.cache.upsert_tool_call(
                                    data.step_index,
                                    tool_name,
                                    state,
                                    args.as_deref(),
                                    output.as_deref(),
                                );
                                self.status_text = format!("Tool: {}", tool_name);
                                self.dirty = true;
                            }
                        }
                    }
                    StreamEvent::Result(res) => {
                        self.waiting_for_response = false;
                        let fallback = res.result.as_ref().and_then(|r| r.response.as_deref());
                        let duration = res
                            .result
                            .as_ref()
                            .and_then(|r| r.duration_seconds)
                            .or_else(|| {
                                self.turn_started_at
                                    .take()
                                    .map(|t| t.elapsed().as_secs_f64())
                            });
                        let tokens = res
                            .result
                            .as_ref()
                            .and_then(|r| r.usage.as_ref())
                            .map(|u| u.output_tokens);
                        let meta = crate::ui::cache::TurnMeta {
                            mode: self.active_mode_name(),
                            model: self.active_model_name(),
                            duration_seconds: duration,
                            output_tokens: tokens,
                        };
                        self.cache
                            .consolidate_active_turn_with_meta(fallback, Some(meta));
                        self.status_text = String::from("Ready");
                        self.dirty = true;
                    }
                    StreamEvent::Error(err) => {
                        self.waiting_for_response = false;
                        let msg = err
                            .error
                            .or(err.message)
                            .unwrap_or_else(|| String::from("Unknown error"));
                        self.cache.push_error(&msg);
                        self.status_text = String::from("Error received");
                        self.dirty = true;
                    }
                    _ => {}
                },
                EngineEvent::Stderr(line) => {
                    self.status_text = format!("stderr: {}", line);
                    self.dirty = true;
                }
                EngineEvent::ProtocolError { message, .. } => {
                    self.waiting_for_response = false;
                    self.cache
                        .push_error(&format!("Protocol Error: {}", message));
                    self.status_text = String::from("Protocol Error");
                    self.dirty = true;
                }
                EngineEvent::ProcessExited { code, .. } => {
                    self.waiting_for_response = false;
                    self.status_text = format!("Process exited: {:?}", code);
                    self.dirty = true;
                }
            }
            if let Some(tab) = self.tabs.get_mut(self.active_tab_index) {
                tab.waiting_for_response = self.waiting_for_response;
                tab.status_text = self.status_text.clone();
            }
        } else if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|t| t.session_id == envelope.session_id)
        {
            let (bg_mode, bg_model) = {
                let meta = self.session_manager.get_metadata(&envelope.session_id);
                (
                    meta.and_then(|m| m.mode.clone())
                        .unwrap_or_else(|| "auto".to_string()),
                    meta.and_then(|m| m.model.clone())
                        .unwrap_or_else(|| "gemini-4-argon-high".to_string()),
                )
            };
            match envelope.event {
                EngineEvent::Stream(boxed_ev) => {
                    let turn_completed =
                        tab.handle_background_stream_event(&boxed_ev, &bg_mode, &bg_model);
                    if turn_completed && !self.clipboard.is_isolated() {
                        emit_sensory_notification(
                            "Frontal Session Completed",
                            &format!("Turn completed in {}", tab.title),
                        );
                    }
                    self.dirty = true;
                }
                EngineEvent::ProtocolError { message, .. } => {
                    tab.waiting_for_response = false;
                    tab.cache
                        .push_error(&format!("Protocol Error: {}", message));
                    tab.status_text = String::from("Protocol Error");
                    tab.has_unread = true;
                    self.dirty = true;
                }
                EngineEvent::Stderr(line) => {
                    tab.status_text = format!("stderr: {}", line);
                    self.dirty = true;
                }
                EngineEvent::ProcessExited { .. } => {
                    tab.waiting_for_response = false;
                    tab.status_text = String::from("Process exited");
                    self.dirty = true;
                }
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let padded_area = if area.width > 16 {
            Rect {
                x: area.x.saturating_add(2),
                y: area.y,
                width: area.width.saturating_sub(4),
                height: area.height,
            }
        } else {
            area
        };

        let input_lines = (self.editor.line_count() as u16).clamp(1, 8);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(input_lines + 2),
                Constraint::Length(1),
            ])
            .split(padded_area);

        let tab_bar_area = chunks[0];
        let _spacer_area = chunks[1];
        let viewport_area = chunks[2];
        let input_area = chunks[3];
        let footer_area = chunks[4];
        self.viewport_bounds = (
            viewport_area.x,
            viewport_area.y,
            viewport_area.width,
            viewport_area.height,
        );

        const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let spinner_char = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];

        let mut tab_spans = Vec::new();
        for (i, tab) in self.tabs.iter().enumerate() {
            let is_active = i == self.active_tab_index;
            let num = i + 1;
            let unread_marker = if tab.has_unread { "*" } else { "" };
            let is_waiting = if is_active {
                self.waiting_for_response
            } else {
                tab.waiting_for_response
            };
            let spinner = if is_waiting {
                format!("{} ", spinner_char)
            } else {
                String::new()
            };
            let dot = if is_active { "● " } else { "○ " };
            let dot_color = if is_active {
                TAB_ACTIVE_DOT
            } else {
                TEXT_MUTED
            };
            let text_color = if is_active {
                TEXT_PRIMARY
            } else if tab.has_unread {
                Color::Yellow
            } else {
                TEXT_MUTED
            };
            tab_spans.push(Span::styled(dot, Style::default().fg(dot_color)));
            tab_spans.push(Span::styled(
                format!("{}: {}{}{}", num, spinner, tab.title, unread_marker),
                Style::default().fg(text_color).add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ));
            tab_spans.push(Span::raw("   "));
        }
        let tab_bar_widget = Paragraph::new(Line::from(tab_spans));
        frame.render_widget(tab_bar_widget, tab_bar_area);

        let total_lines = self.cache.ensure_visual_lines(viewport_area.width);
        let total_with_spinner = total_lines + if self.waiting_for_response { 1 } else { 0 };
        self.viewport
            .sync_dimensions(total_with_spinner, viewport_area.height as usize);
        let mut visible_lines = self.viewport.visible_slice(&self.cache);

        if self.waiting_for_response {
            const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];
            visible_lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!("{} ", spinner),
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Thinking / Waiting for response...",
                    Style::default()
                        .fg(TEXT_MUTED)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        let viewport_widget = Paragraph::new(visible_lines);
        frame.render_widget(viewport_widget, viewport_area);

        let model_name = self.active_model_name();
        let mode_name = self.active_mode_name();
        let effort_name = self.active_effort_name();
        let input_title = if model_name.ends_with(&format!("-{}", effort_name)) {
            format!(" Input [{}] · {} ", mode_name, model_name)
        } else {
            format!(" Input [{}] · {} ({}) ", mode_name, model_name, effort_name)
        };
        let display_content = if !self.editor.is_empty() {
            self.editor.text()
        } else {
            self.input.content().to_string()
        };

        let (cur_line, cur_col) = if !self.editor.is_empty() {
            self.editor.cursor_coordinates()
        } else {
            (0, self.input.cursor_char_idx())
        };

        let scroll_y = if cur_line >= 8 {
            (cur_line - 7) as u16
        } else {
            0
        };

        let mut input_widget = Paragraph::new(display_content).block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(Style::default().fg(BORDER_SUBTLE))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        );
        if scroll_y > 0 {
            input_widget = input_widget.scroll((scroll_y, 0));
        }
        frame.render_widget(input_widget, input_area);

        let footer_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(10), Constraint::Length(38)])
            .split(footer_area);

        let cwd_widget = Paragraph::new(Line::from(vec![Span::styled(
            &self.cwd_display,
            Style::default().fg(TEXT_MUTED),
        )]));
        frame.render_widget(cwd_widget, footer_chunks[0]);

        let shortcuts_widget = Paragraph::new(Line::from(vec![Span::styled(
            "Ctrl+P: Palette · Ctrl+C: Interrupt",
            Style::default().fg(TEXT_MUTED),
        )]))
        .alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(shortcuts_widget, footer_chunks[1]);

        let cursor_x = input_area
            .x
            .saturating_add(2)
            .saturating_add(cur_col as u16);
        let cursor_y = input_area
            .y
            .saturating_add(1)
            .saturating_add(cur_line.saturating_sub(scroll_y as usize) as u16);
        if cursor_x
            < input_area
                .x
                .saturating_add(input_area.width.saturating_sub(2))
            && cursor_y
                < input_area
                    .y
                    .saturating_add(input_area.height.saturating_sub(1))
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        if self.popover.is_visible() {
            let item_count = self.popover.items.len().min(8) as u16;
            let popover_height = item_count + 2;
            let popover_width = (input_area.width.saturating_sub(4)).min(60);
            let popover_y = input_area.y.saturating_sub(popover_height);
            let popover_rect = Rect {
                x: input_area.x.saturating_add(2),
                y: popover_y,
                width: popover_width,
                height: popover_height,
            }
            .intersection(area);

            frame.render_widget(Clear, popover_rect);

            let mut popover_lines = Vec::new();
            for (idx, item) in self
                .popover
                .items
                .iter()
                .enumerate()
                .take(item_count as usize)
            {
                let is_sel = idx == self.popover.selected_idx;
                let (prefix, style) = if is_sel {
                    (
                        " > ",
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("   ", Style::default().fg(Color::White))
                };
                popover_lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&item.label, style),
                ]));
            }

            let title = match self.popover.kind {
                TriggerKind::Command => " Commands (Tab/Enter: apply, Esc: close) ",
                TriggerKind::File => " Files (Tab/Enter: apply, Esc: close) ",
            };

            let popover_widget = Paragraph::new(popover_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(popover_widget, popover_rect);
        }

        if !self.modals.is_empty() {
            self.modals.render(frame, area);
        }
    }
}
