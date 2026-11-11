use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendKind {
    #[default]
    Agy,
    OpenCode,
}

impl BackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Agy => "agy",
            Self::OpenCode => "opencode",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "opencode" | "open-code" => Self::OpenCode,
            _ => Self::Agy,
        }
    }
}

pub const DEFAULT_MODEL: &str = "gemini-3.8-flash-high";
pub const DEFAULT_EFFORT: &str = "high";
pub const DEFAULT_MODE: &str = "auto";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserConfig {
    pub model: String,
    pub effort: String,
    pub mode: String,
    pub backend: BackendKind,
    pub isolated: bool,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self::load()
    }
}

impl UserConfig {
    pub fn new_isolated() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            effort: DEFAULT_EFFORT.to_string(),
            mode: DEFAULT_MODE.to_string(),
            backend: BackendKind::Agy,
            isolated: true,
        }
    }

    pub fn default_config() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            effort: DEFAULT_EFFORT.to_string(),
            mode: DEFAULT_MODE.to_string(),
            backend: BackendKind::Agy,
            isolated: false,
        }
    }

    pub fn config_path() -> Option<PathBuf> {
        env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".frontal").join("config.toml"))
    }

    pub fn load() -> Self {
        let mut config = Self::default_config();
        if let Some(path) = Self::config_path() {
            if let Ok(content) = fs::read_to_string(&path) {
                config.parse_content(&content);
            }
        }
        config
    }

    pub fn parse_content(&mut self, content: &str) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = trimmed.split_once('=') {
                let k = key.trim();
                let v = val.trim().trim_matches('"').trim_matches('\'').trim();
                match k {
                    "model" if !v.is_empty() => {
                        self.model = v.to_string();
                    }
                    "effort" if !v.is_empty() => {
                        self.effort = v.to_string();
                    }
                    "mode" if !v.is_empty() => {
                        self.mode = v.to_string();
                    }
                    "backend" if !v.is_empty() => {
                        self.backend = BackendKind::parse(v);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn save(&self) -> io::Result<()> {
        if self.isolated {
            return Ok(());
        }
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let payload = format!(
                "model = \"{}\"\neffort = \"{}\"\nmode = \"{}\"\nbackend = \"{}\"\n",
                self.model,
                self.effort,
                self.mode,
                self.backend.as_str()
            );
            fs::write(path, payload)?;
        }
        Ok(())
    }
}
