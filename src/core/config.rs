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
    pub context_windows: Vec<(String, u64)>,
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
            context_windows: Vec::new(),
        }
    }

    pub fn default_config() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            effort: DEFAULT_EFFORT.to_string(),
            mode: DEFAULT_MODE.to_string(),
            backend: BackendKind::Agy,
            isolated: false,
            context_windows: Vec::new(),
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
        let mut section: Option<String> = None;
        for line in content.lines() {
            let trimmed = strip_comment(line).trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                section = Some(trimmed[1..trimmed.len() - 1].trim().to_lowercase());
                continue;
            }
            let Some((key, val)) = trimmed.split_once('=') else {
                continue;
            };
            let k = key.trim().trim_matches('"').trim_matches('\'');
            let v = val.trim().trim_matches('"').trim_matches('\'');
            if section.as_deref() == Some("context_windows") {
                if let Some(tokens) = parse_token_count(v) {
                    self.set_context_window(k, tokens);
                }
                continue;
            }
            if section.is_some() {
                continue;
            }
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

    pub fn set_context_window(&mut self, model: &str, tokens: u64) {
        let key = model.trim();
        if key.is_empty() || tokens == 0 {
            return;
        }
        if let Some(entry) = self
            .context_windows
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(key))
        {
            entry.1 = tokens;
        } else {
            self.context_windows.push((key.to_string(), tokens));
        }
    }

    pub fn to_toml(&self) -> String {
        let mut payload = format!(
            "model = \"{}\"\neffort = \"{}\"\nmode = \"{}\"\nbackend = \"{}\"\n",
            self.model,
            self.effort,
            self.mode,
            self.backend.as_str()
        );
        if !self.context_windows.is_empty() {
            payload.push_str("\n[context_windows]\n");
            for (model, tokens) in &self.context_windows {
                payload.push_str(&format!("\"{}\" = {}\n", model, tokens));
            }
        }
        payload
    }

    pub fn save(&self) -> io::Result<()> {
        if self.isolated {
            return Ok(());
        }
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, self.to_toml())?;
        }
        Ok(())
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (index, ch) in line.char_indices() {
        if ch == '"' || ch == '\'' {
            if quote == Some(ch) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
        } else if ch == '#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}

fn parse_token_count(value: &str) -> Option<u64> {
    let cleaned: String = value
        .chars()
        .filter(|ch| *ch != '_' && *ch != ',')
        .collect();
    cleaned.parse::<u64>().ok().filter(|tokens| *tokens > 0)
}
