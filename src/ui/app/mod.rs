mod commands;
mod events;
mod render;
mod stream;
mod tabs;

use std::ops::{Deref, DerefMut};

use crate::protocol::SessionId;
use crate::session::SessionManager;
use crate::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use crate::ui::clipboard::SystemClipboard;
use crate::ui::history::PromptHistory;
use crate::ui::modal::ModalStack;
use crate::ui::tab::TabState;

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
    pub popover: AutocompletePopover,
    pub search_worker: FileSearchWorker,
    pub history: PromptHistory,
    pub dirty: bool,
    pub should_quit: bool,
    pub spinner_tick: u64,
    pub tabs: Vec<TabState>,
    pub active_tab_index: usize,
    pub modals: ModalStack,
    pub clipboard: SystemClipboard,
    pub cwd_display: String,
    pub available_models: Vec<(String, String)>,
    pub opencode_models: Vec<(String, String)>,
    pub user_config: crate::config::UserConfig,
    pub viewport_bounds: (u16, u16, u16, u16),
    pub last_ctrl_c_at: Option<std::time::Instant>,
    pub pending_engine_prompts: std::collections::VecDeque<(crate::protocol::SessionId, String)>,
}

impl Deref for App {
    type Target = TabState;

    fn deref(&self) -> &Self::Target {
        &self.tabs[self.active_tab_index]
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tabs[self.active_tab_index]
    }
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

        let backend = session_manager
            .get_engine_config(&active_session)
            .map(|config| config.backend)
            .unwrap_or(crate::config::BackendKind::Agy);
        let mut initial_tab = TabState::new(active_session.clone(), String::from("Session 1"));
        initial_tab.backend = backend;
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
            popover: AutocompletePopover::new(),
            search_worker,
            history,
            dirty: true,
            should_quit: false,
            spinner_tick: 0,
            tabs: vec![initial_tab],
            active_tab_index: 0,
            modals: ModalStack::new(),
            clipboard: SystemClipboard::new(),
            cwd_display,
            available_models,
            opencode_models: Vec::new(),
            user_config: crate::config::UserConfig::load(),
            viewport_bounds: (0, 0, 80, 20),
            last_ctrl_c_at: None,
            pending_engine_prompts: std::collections::VecDeque::new(),
        }
    }

    pub fn new_isolated(session_manager: SessionManager, active_session: SessionId) -> Self {
        let history = PromptHistory::open_in_memory().unwrap();
        let search_worker = FileSearchWorker::new();
        let backend = session_manager
            .get_engine_config(&active_session)
            .map(|config| config.backend)
            .unwrap_or(crate::config::BackendKind::Agy);
        let mut initial_tab = TabState::new(active_session.clone(), String::from("Session 1"));
        initial_tab.backend = backend;
        let cwd_display = String::from("~/test");
        let available_models = crate::transport::EngineConfig::default_fallback_models();

        Self {
            session_manager,
            active_session,
            popover: AutocompletePopover::new(),
            search_worker,
            history,
            dirty: true,
            should_quit: false,
            spinner_tick: 0,
            tabs: vec![initial_tab],
            active_tab_index: 0,
            modals: ModalStack::new(),
            clipboard: SystemClipboard::new_isolated(),
            cwd_display,
            available_models,
            opencode_models: Vec::new(),
            user_config: crate::config::UserConfig::new_isolated(),
            viewport_bounds: (0, 0, 80, 20),
            last_ctrl_c_at: None,
            pending_engine_prompts: std::collections::VecDeque::new(),
        }
    }

    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab_index]
    }

    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn set_available_models(&mut self, models: Vec<(String, String)>) {
        self.available_models = models;
    }

    pub fn set_opencode_models(&mut self, models: Vec<(String, String)>) {
        self.opencode_models = models;
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
}
