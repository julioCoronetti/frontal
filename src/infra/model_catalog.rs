use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use crate::model_catalog::ModelCatalog;

const CATALOG_KEY: &str = "models-dev:v2:catalog";

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

pub fn load_default() -> ModelCatalog {
    default_path()
        .filter(|path| path.is_file())
        .and_then(|path| load_from_path(&path))
        .unwrap_or_default()
}

pub fn load_from_path(path: &Path) -> Option<ModelCatalog> {
    if !path.is_file() {
        return None;
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
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
    ModelCatalog::parse_catalog(&raw)
}
