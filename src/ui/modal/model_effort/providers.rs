use crate::config::BackendKind;

use super::{ModelEffortStep, ModelItem};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderItem {
    pub id: String,
    pub name: String,
    pub count: usize,
}

impl super::ModelEffortModal {
    pub(crate) fn rebuild_providers(&mut self) {
        let mut providers: Vec<ProviderItem> = Vec::new();
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
                    name: id.clone(),
                    id,
                    count: 1,
                });
            }
        }
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        self.providers = providers;
    }

    pub(crate) fn scope_models_to_provider(&mut self) {
        self.models = match &self.provider {
            Some(provider) => {
                let prefix = format!("{}/", provider);
                self.opencode_models
                    .iter()
                    .filter(|model| model.id.starts_with(&prefix))
                    .cloned()
                    .collect()
            }
            None => self.opencode_models.clone(),
        };
        self.selected_model_idx = 0;
        let idx = Self::default_effort_for_model(self.current_supported_efforts()).0;
        self.selected_effort_idx = idx;
    }

    pub(crate) fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn showing_providers(&self) -> bool {
        self.backend == BackendKind::OpenCode
            && self.step == ModelEffortStep::SelectProvider
            && !self.searching()
    }

    pub(crate) fn active_pool(&self) -> &[ModelItem] {
        if self.searching() {
            match self.backend {
                BackendKind::Agy => &self.agy_models,
                BackendKind::OpenCode => &self.opencode_models,
            }
        } else {
            &self.models
        }
    }

    pub fn set_opencode_models(&mut self, raw: Vec<(String, String)>) {
        let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"]
            .iter()
            .map(|effort| effort.to_string())
            .collect();
        let normalized: Vec<ModelItem> = raw
            .into_iter()
            .map(|(id, name)| ModelItem {
                id,
                name,
                supported_efforts: efforts.clone(),
            })
            .collect();
        self.opencode_models = normalized;
        self.rebuild_providers();
        if self.backend == BackendKind::OpenCode {
            if self.provider.is_some() {
                self.scope_models_to_provider();
            } else {
                self.models = self.opencode_models.clone();
            }
        }
    }
}
