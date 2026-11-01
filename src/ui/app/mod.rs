mod commands;
mod events;
mod render;
mod stream;
mod tabs;

use crate::protocol::SessionId;
use crate::session::SessionManager;
use crate::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use crate::ui::cache::RenderCache;
use crate::ui::clipboard::SystemClipboard;
use crate::ui::editor::{EditorSnapshot, PromptEditor};
use crate::ui::history::PromptHistory;
use crate::ui::input_stub::MinimalInputStub;
use crate::ui::modal::ModalStack;
use crate::ui::tab::TabState;
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
    pub user_config: crate::config::UserConfig,
    pub viewport_bounds: (u16, u16, u16, u16),
    pub turn_started_at: Option<std::time::Instant>,
    pub last_ctrl_c_at: Option<std::time::Instant>,
    pub last_turn_usage: Option<crate::protocol::TokenUsage>,
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
            user_config: crate::config::UserConfig::load(),
            viewport_bounds: (0, 0, 80, 20),
            turn_started_at: None,
            last_ctrl_c_at: None,
            last_turn_usage: None,
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
            user_config: crate::config::UserConfig::new_isolated(),
            viewport_bounds: (0, 0, 80, 20),
            turn_started_at: None,
            last_ctrl_c_at: None,
            last_turn_usage: None,
        }
    }

    pub fn set_available_models(&mut self, models: Vec<(String, String)>) {
        self.available_models = models;
    }

    pub fn active_model_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.model.clone())
            .unwrap_or_else(|| self.user_config.model.clone())
    }

    pub fn active_mode_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.mode.clone())
            .unwrap_or_else(|| self.user_config.mode.clone())
    }

    pub fn active_effort_name(&self) -> String {
        self.session_manager
            .get_metadata(&self.active_session)
            .and_then(|m| m.effort.clone())
            .unwrap_or_else(|| self.user_config.effort.clone())
    }

    pub(crate) fn sync_editor_to_input(&mut self) {
        self.input.clear();
        self.input.insert_str(&self.editor.text());
        self.draft_snapshot = None;
    }
}
