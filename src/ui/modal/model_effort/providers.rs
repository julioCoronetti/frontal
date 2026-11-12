use crate::config::BackendKind;

use super::{ModelEffortStep, ModelItem};

pub const AGY_PROVIDER_ID: &str = "agy";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderItem {
    pub id: String,
    pub name: String,
    pub count: usize,
    pub backend: BackendKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListEntry {
    Provider(usize),
    Model { backend: BackendKind, index: usize },
}

impl super::ModelEffortModal {
    pub(crate) fn rebuild_providers(&mut self) {
        let mut providers = vec![ProviderItem {
            id: AGY_PROVIDER_ID.to_string(),
            name: crate::ui::labels::provider_display_name(AGY_PROVIDER_ID),
            count: self.agy_models.len(),
            backend: BackendKind::Agy,
        }];
        for model in &self.opencode_models {
            let id = model
                .id
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
                .unwrap_or_else(|| model.id.clone());
            if let Some(existing) = providers.iter_mut().find(|item| item.id == id) {
                existing.count += 1;
            } else {
                providers.push(ProviderItem {
                    name: crate::ui::labels::provider_display_name(&id),
                    id,
                    count: 1,
                    backend: BackendKind::OpenCode,
                });
            }
        }
        providers.sort_by(|a, b| {
            (a.backend != BackendKind::Agy, a.id.as_str())
                .cmp(&(b.backend != BackendKind::Agy, b.id.as_str()))
        });
        self.providers = providers;
    }

    pub(crate) fn model_pool(&self, backend: BackendKind) -> &[ModelItem] {
        match backend {
            BackendKind::Agy => &self.agy_models,
            BackendKind::OpenCode => &self.opencode_models,
        }
    }

    pub(crate) fn visible_entries(&self) -> Vec<ListEntry> {
        if self.showing_providers() {
            return (0..self.providers.len()).map(ListEntry::Provider).collect();
        }
        if self.searching() {
            let query = self.query.trim().to_lowercase();
            let mut entries = Vec::new();
            for (index, model) in self.agy_models.iter().enumerate() {
                if Self::matches_query(model, &query) {
                    entries.push(ListEntry::Model {
                        backend: BackendKind::Agy,
                        index,
                    });
                }
            }
            for (index, model) in self.opencode_models.iter().enumerate() {
                if Self::matches_query(model, &query) {
                    entries.push(ListEntry::Model {
                        backend: BackendKind::OpenCode,
                        index,
                    });
                }
            }
            entries.sort_by(|a, b| {
                let name_a = match a {
                    ListEntry::Model { backend, index } => &self.model_pool(*backend)[*index].name,
                    ListEntry::Provider(_) => unreachable!(),
                };
                let name_b = match b {
                    ListEntry::Model { backend, index } => &self.model_pool(*backend)[*index].name,
                    ListEntry::Provider(_) => unreachable!(),
                };
                name_a.cmp(name_b)
            });
            return entries;
        }
        let pool = match self.backend {
            BackendKind::Agy => &self.agy_models,
            BackendKind::OpenCode => {
                return self
                    .opencode_models
                    .iter()
                    .enumerate()
                    .filter(|(_, model)| match &self.provider {
                        Some(provider) => model.id.starts_with(&format!("{}/", provider)),
                        None => true,
                    })
                    .map(|(index, _)| ListEntry::Model {
                        backend: BackendKind::OpenCode,
                        index,
                    })
                    .collect();
            }
        };
        (0..pool.len())
            .map(|index| ListEntry::Model {
                backend: self.backend,
                index,
            })
            .collect()
    }

    fn matches_query(model: &ModelItem, query: &str) -> bool {
        model.id.to_lowercase().contains(query) || model.name.to_lowercase().contains(query)
    }

    pub(crate) fn selected_entry(&self) -> Option<ListEntry> {
        self.visible_entries().get(self.selected_model_idx).copied()
    }

    pub(crate) fn selected_model(&self) -> Option<&ModelItem> {
        match self.selected_entry()? {
            ListEntry::Model { backend, index } => Some(&self.model_pool(backend)[index]),
            ListEntry::Provider(_) => None,
        }
    }

    pub(crate) fn provider_label(&self, backend: BackendKind, model_id: &str) -> String {
        let id = match backend {
            BackendKind::Agy => AGY_PROVIDER_ID.to_string(),
            BackendKind::OpenCode => model_id
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
                .unwrap_or_default(),
        };
        self.providers
            .iter()
            .find(|provider| provider.id == id)
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| crate::ui::labels::provider_display_name(&id))
    }

    pub(crate) fn activate_provider(&mut self, index: usize) {
        let Some(provider) = self.providers.get(index).cloned() else {
            return;
        };
        self.backend = provider.backend;
        self.provider = Some(provider.id.clone());
        self.step = ModelEffortStep::SelectModel;
        self.query.clear();
        self.selected_model_idx = 0;
        self.scope_models_to_provider();
    }

    pub(crate) fn back_to_providers(&mut self) {
        self.step = ModelEffortStep::SelectProvider;
        self.query.clear();
        self.selected_model_idx = self
            .providers
            .iter()
            .position(|provider| provider.backend == self.backend)
            .unwrap_or(0);
    }

    pub(crate) fn scope_models_to_provider(&mut self) {
        self.models = match (self.backend, &self.provider) {
            (BackendKind::Agy, _) => self.agy_models.clone(),
            (BackendKind::OpenCode, Some(provider)) => {
                let prefix = format!("{}/", provider);
                self.opencode_models
                    .iter()
                    .filter(|model| model.id.starts_with(&prefix))
                    .cloned()
                    .collect()
            }
            (BackendKind::OpenCode, None) => self.opencode_models.clone(),
        };
        self.selected_model_idx = 0;
        let idx = Self::default_effort_for_model(self.current_supported_efforts()).0;
        self.selected_effort_idx = idx;
    }

    pub(crate) fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn showing_providers(&self) -> bool {
        self.step == ModelEffortStep::SelectProvider && !self.searching()
    }

    pub fn set_opencode_models(&mut self, raw: Vec<(String, String)>) {
        let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"]
            .iter()
            .map(|effort| effort.to_string())
            .collect();
        self.opencode_models = raw
            .into_iter()
            .map(|(id, _name)| ModelItem {
                name: crate::ui::labels::model_display_name(&id),
                id,
                supported_efforts: efforts.clone(),
            })
            .collect();
        self.rebuild_providers();
        self.scope_models_to_provider();
    }
}
