mod render;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::BackendKind;
use crate::ui::modal::ModalAction;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelEffortStep {
    SelectModel,
    SelectEffort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
    pub supported_efforts: Vec<String>,
}

pub struct ModelEffortModal {
    pub step: ModelEffortStep,
    pub backend: BackendKind,
    pub query: String,
    pub models: Vec<ModelItem>,
    pub agy_models: Vec<ModelItem>,
    pub opencode_models: Vec<ModelItem>,
    pub fallback_efforts: Vec<String>,
    pub selected_model_idx: usize,
    pub selected_effort_idx: usize,
    pub current_model: Option<String>,
    pub current_effort: Option<String>,
}

impl Default for ModelEffortModal {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelEffortModal {
    pub fn new() -> Self {
        Self::with_models(crate::transport::EngineConfig::default_fallback_models())
    }

    pub fn with_models(models: Vec<(String, String)>) -> Self {
        let items = Self::normalize_catalog(models);
        let fallback = vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ];
        let initial_effort_idx = items
            .first()
            .map(|m| Self::default_effort_for_model(&m.supported_efforts).0)
            .unwrap_or(0);
        Self {
            step: ModelEffortStep::SelectModel,
            backend: BackendKind::Agy,
            query: String::new(),
            agy_models: items.clone(),
            opencode_models: Vec::new(),
            models: items,
            fallback_efforts: fallback,
            selected_model_idx: 0,
            selected_effort_idx: initial_effort_idx,
            current_model: None,
            current_effort: None,
        }
    }

    pub fn with_initial_selection(
        mut self,
        current_model: Option<String>,
        current_effort: Option<String>,
    ) -> Self {
        if let Some(ref cur_m) = current_model {
            if let Some(pos) = self
                .models
                .iter()
                .position(|m| &m.id == cur_m || cur_m.starts_with(&format!("{}-", m.id)))
            {
                self.selected_model_idx = pos;
            }
        }
        let effs = self.current_supported_efforts();
        if let Some(ref cur_e) = current_effort {
            if let Some(pos) = effs.iter().position(|e| e == cur_e) {
                self.selected_effort_idx = pos;
            } else {
                self.selected_effort_idx = Self::default_effort_for_model(effs).0;
            }
        } else {
            self.selected_effort_idx = Self::default_effort_for_model(effs).0;
        }
        self.current_model = current_model;
        self.current_effort = current_effort;
        self
    }

    fn parse_model_id_and_effort(raw_id: &str, raw_name: &str) -> (String, String, Option<String>) {
        let known_efforts = [
            ("-low", " (Low)", "low"),
            ("-medium", " (Medium)", "medium"),
            ("-high", " (High)", "high"),
            ("-xhigh", " (XHigh)", "xhigh"),
            ("-max", " (Max)", "max"),
        ];

        for &(id_sfx, name_sfx, effort) in &known_efforts {
            if let Some(base_id) = raw_id.strip_suffix(id_sfx) {
                let base_name = raw_name.strip_suffix(name_sfx).unwrap_or(raw_name);
                return (
                    base_id.to_string(),
                    base_name.to_string(),
                    Some(effort.to_string()),
                );
            }
        }

        (raw_id.to_string(), raw_name.to_string(), None)
    }

    pub fn normalize_catalog(raw_models: Vec<(String, String)>) -> Vec<ModelItem> {
        let default_efforts = vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ];

        let mut result: Vec<ModelItem> = Vec::new();

        for (raw_id, raw_name) in raw_models {
            let (base_id, base_name, effort_opt) =
                Self::parse_model_id_and_effort(&raw_id, &raw_name);
            if let Some(existing) = result.iter_mut().find(|m| m.id == base_id) {
                if let Some(eff) = effort_opt {
                    if !existing.supported_efforts.contains(&eff) {
                        existing.supported_efforts.push(eff);
                        Self::sort_efforts(&mut existing.supported_efforts);
                    }
                }
            } else {
                let supported = match effort_opt {
                    Some(eff) => vec![eff],
                    None => default_efforts.clone(),
                };
                result.push(ModelItem {
                    id: base_id,
                    name: base_name,
                    supported_efforts: supported,
                });
            }
        }

        result
    }

    fn sort_efforts(efforts: &mut [String]) {
        let order = ["low", "medium", "high", "xhigh", "max"];
        efforts.sort_by_key(|e| {
            order
                .iter()
                .position(|&o| o == e.as_str())
                .unwrap_or(order.len())
        });
    }

    pub fn backend_label(&self) -> &'static str {
        match self.backend {
            BackendKind::Agy => "Agy",
            BackendKind::OpenCode => "OpenCode",
        }
    }

    pub fn toggle_backend(&mut self) {
        let next = match self.backend {
            BackendKind::Agy => BackendKind::OpenCode,
            BackendKind::OpenCode => BackendKind::Agy,
        };
        self.set_backend(next);
    }

    pub fn set_backend(&mut self, backend: BackendKind) {
        if self.backend == backend {
            return;
        }
        match self.backend {
            BackendKind::Agy => self.agy_models = self.models.clone(),
            BackendKind::OpenCode => self.opencode_models = self.models.clone(),
        }
        self.backend = backend;
        self.models = match backend {
            BackendKind::Agy => self.agy_models.clone(),
            BackendKind::OpenCode => self.opencode_models.clone(),
        };
        self.query.clear();
        self.selected_model_idx = 0;
        let idx = Self::default_effort_for_model(self.current_supported_efforts()).0;
        self.selected_effort_idx = idx;
    }

    pub fn set_opencode_models(&mut self, raw: Vec<(String, String)>) {
        let normalized = Self::normalize_catalog(raw);
        if self.backend == BackendKind::OpenCode {
            self.models = normalized.clone();
        }
        self.opencode_models = normalized;
    }

    pub fn current_supported_efforts(&self) -> &[String] {
        let filtered = self.filtered_indices();
        if let Some(&idx) = filtered.get(self.selected_model_idx) {
            return &self.models[idx].supported_efforts;
        }
        &self.fallback_efforts
    }

    pub fn default_effort_for_model(efforts: &[String]) -> (usize, String) {
        if let Some(pos) = efforts.iter().position(|e| e == "high") {
            (pos, "high".to_string())
        } else if let Some(pos) = efforts.iter().position(|e| e == "medium") {
            (pos, "medium".to_string())
        } else if let Some(first) = efforts.first() {
            (0, first.clone())
        } else {
            (0, "high".to_string())
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                if q.is_empty() {
                    true
                } else {
                    m.id.to_lowercase().contains(&q) || m.name.to_lowercase().contains(&q)
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn effort_description(effort: &str) -> &'static str {
        match effort {
            "low" => "Fastest response, minimal reasoning — best for straightforward tasks",
            "medium" => "Balanced speed and reasoning — recommended for most tasks",
            "high" => "Deepest reasoning for complex problems — slower but strongest",
            "xhigh" => "Extended reasoning budget for difficult coding challenges",
            "max" => "Maximum reasoning capability and exhaustive verification",
            _ => "Configured reasoning effort level",
        }
    }

    fn sync_effort_after_model_change(&mut self, prev_effort: Option<String>) {
        let effs = self.current_supported_efforts();
        if let Some(prev) = prev_effort {
            if let Some(pos) = effs.iter().position(|e| e == &prev) {
                self.selected_effort_idx = pos;
                return;
            }
        }
        let (def_idx, _) = Self::default_effort_for_model(effs);
        self.selected_effort_idx = def_idx;
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let prev_eff = self
            .current_supported_efforts()
            .get(self.selected_effort_idx)
            .cloned()
            .or_else(|| self.current_effort.clone());
        self.query = query.into();
        self.selected_model_idx = 0;
        self.sync_effort_after_model_change(prev_eff);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        let max_model = self.filtered_indices().len().saturating_sub(1);
        let max_eff = self.current_supported_efforts().len().saturating_sub(1);

        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.selected_model_idx = self.selected_model_idx.saturating_sub(1);
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            KeyCode::Down => {
                if self.selected_model_idx < max_model {
                    let prev_eff = self
                        .current_supported_efforts()
                        .get(self.selected_effort_idx)
                        .cloned();
                    self.selected_model_idx += 1;
                    self.sync_effort_after_model_change(prev_eff);
                }
                ModalAction::None
            }
            KeyCode::Left => {
                self.selected_effort_idx = self.selected_effort_idx.saturating_sub(1);
                ModalAction::None
            }
            KeyCode::Right => {
                self.selected_effort_idx = (self.selected_effort_idx + 1).min(max_eff);
                ModalAction::None
            }
            KeyCode::Tab => {
                let effs = self.current_supported_efforts();
                if !effs.is_empty() {
                    self.selected_effort_idx = (self.selected_effort_idx + 1) % effs.len();
                }
                ModalAction::None
            }
            KeyCode::BackTab => {
                let effs = self.current_supported_efforts();
                if !effs.is_empty() {
                    self.selected_effort_idx =
                        (self.selected_effort_idx + effs.len() - 1) % effs.len();
                }
                ModalAction::None
            }
            KeyCode::Enter => {
                let filtered = self.filtered_indices();
                if let Some(&real_idx) = filtered.get(self.selected_model_idx) {
                    let model = self.models[real_idx].id.clone();
                    let efforts = self.current_supported_efforts();
                    let effort = efforts
                        .get(self.selected_effort_idx)
                        .cloned()
                        .unwrap_or_else(|| "high".to_string());
                    ModalAction::SelectModelEffort { model, effort }
                } else {
                    ModalAction::None
                }
            }
            KeyCode::F(2) => {
                self.toggle_backend();
                ModalAction::None
            }
            KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_backend();
                ModalAction::None
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.query.push(c);
                self.selected_model_idx = 0;
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            KeyCode::Backspace => {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.query.pop();
                self.selected_model_idx = 0;
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }
}
