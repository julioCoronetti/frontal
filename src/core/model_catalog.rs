use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCatalog {
    limits: HashMap<String, u64>,
}

impl ModelCatalog {
    pub fn parse_catalog(raw: &str) -> Option<Self> {
        let envelope: serde_json::Value = serde_json::from_str(raw).ok()?;
        let body = envelope.get("body")?.as_str()?;
        let catalog: serde_json::Value = serde_json::from_str(body).ok()?;
        let providers = catalog.as_object()?;
        let mut limits = HashMap::new();
        for (provider_id, provider) in providers {
            let Some(models) = provider.get("models").and_then(|models| models.as_object()) else {
                continue;
            };
            for (model_id, model) in models {
                let Some(context) = model
                    .get("limit")
                    .and_then(|limit| limit.get("context"))
                    .and_then(|context| context.as_u64())
                else {
                    continue;
                };
                if context == 0 {
                    continue;
                }
                limits.insert(
                    format!("{}/{}", provider_id, model_id).to_lowercase(),
                    context,
                );
                limits.entry(model_id.to_lowercase()).or_insert(context);
            }
        }
        if limits.is_empty() {
            return None;
        }
        Some(Self { limits })
    }

    pub fn get(&self, model_key: &str) -> Option<u64> {
        self.limits.get(model_key).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.limits.is_empty()
    }
}
