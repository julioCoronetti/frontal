use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::config::UserConfig;

pub fn config_path() -> Option<PathBuf> {
    env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".frontal").join("config.toml"))
}

pub fn load() -> UserConfig {
    let mut config = UserConfig::default_config();
    if let Some(path) = config_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            config.parse_content(&content);
            if config.needs_repair {
                let _ = save(&config);
            }
        }
    }
    config
}

pub fn save(config: &UserConfig) -> io::Result<()> {
    if config.isolated {
        return Ok(());
    }
    if let Some(path) = config_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, config.to_toml())?;
    }
    Ok(())
}
