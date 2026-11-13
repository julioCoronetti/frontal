use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

const CATALOG_KEY: &str = "models-dev:v2:catalog";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelCatalog {
    limits: HashMap<String, u64>,
}

impl ModelCatalog {
    pub fn load_default() -> Self {
        Self::default_path()
            .filter(|path| path.is_file())
            .and_then(|path| Self::load_from_path(&path))
            .unwrap_or_default()
    }

    pub fn default_path() -> Option<PathBuf> {
        if let Ok(data_home) = env::var("XDG_DATA_HOME") {
            let candidate = PathBuf::from(data_home)
                .join("opencode")
                .join("opencode.db");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".local/share/opencode/opencode.db"))
    }

    pub fn load_from_path(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let connection =
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        connection.busy_timeout(Duration::from_millis(2_000)).ok()?;
        let value: Value = connection
            .query_row(
                "SELECT value FROM kv WHERE key = ?1",
                [CATALOG_KEY],
                |row| row.get(0),
            )
            .ok()?;
        let raw = match value {
            Value::Text(text) => text,
            Value::Blob(bytes) => String::from_utf8(bytes).ok()?,
            _ => return None,
        };
        Self::parse_catalog(&raw)
    }

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
