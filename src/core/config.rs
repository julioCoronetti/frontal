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

    pub fn normalize_mode(raw: &str) -> Option<&'static str> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" => Some("auto"),
            "plan" => Some("plan"),
            "accept-edits" | "accept_edits" => Some("accept-edits"),
            _ => None,
        }
    }

    pub fn supported_modes(&self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::Agy => &[
                ("auto", "Autonomous execution"),
                ("plan", "Read-only planning & architecture"),
                ("accept-edits", "Auto-accept file edits"),
            ],
            Self::OpenCode => &[
                ("auto", "Autonomous execution (Build agent)"),
                ("plan", "Read-only planning (Plan agent)"),
            ],
        }
    }

    pub fn supports_mode(&self, mode: &str) -> bool {
        Self::normalize_mode(mode).is_some_and(|canonical| {
            self.supported_modes()
                .iter()
                .any(|(id, _)| *id == canonical)
        })
    }

    pub fn supported_efforts(&self) -> &'static [&'static str] {
        match self {
            Self::Agy | Self::OpenCode => &["low", "medium", "high", "xhigh", "max"],
        }
    }

    pub fn sanitize_effort(&self, raw: &str) -> Option<String> {
        let token = raw
            .trim()
            .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        self.supported_efforts()
            .iter()
            .copied()
            .find(|&effort| effort == token)
            .map(str::to_string)
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
    pub needs_repair: bool,
}

pub fn sanitize_config_token(raw: &str) -> String {
    raw.trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>()
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase()
}

pub fn sanitize_model_id(raw: &str) -> String {
    let trimmed = raw.trim();
    let (base_raw, variant_raw) = match trimmed.split_once('#') {
        Some((base, variant)) => (base, Some(variant)),
        None => (trimmed, None),
    };
    let base = base_raw
        .chars()
        .filter(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '.' | '_' | ':' | '@' | '+')
        })
        .collect::<String>();
    let base = base
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_string();
    let variant = variant_raw
        .map(|value| {
            value
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_'))
                .collect::<String>()
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
                .to_string()
        })
        .filter(|value| !value.is_empty());
    match variant {
        Some(value) if !base.is_empty() => format!("{}#{}", base, value),
        _ => base,
    }
}

impl Default for UserConfig {
    fn default() -> Self {
        Self::default_config()
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
            needs_repair: false,
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
            needs_repair: false,
        }
    }

    pub fn parse_content(&mut self, content: &str) {
        let mut section: Option<String> = None;
        let mut raw_effort: Option<String> = None;
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
                    let sanitized = sanitize_model_id(v);
                    if !sanitized.is_empty() {
                        if sanitized != v {
                            self.needs_repair = true;
                        }
                        self.model = sanitized;
                    }
                }
                "effort" if !v.is_empty() => {
                    raw_effort = Some(v.to_string());
                }
                "mode" if !v.is_empty() => {
                    let sanitized = BackendKind::normalize_mode(v)
                        .map(str::to_string)
                        .unwrap_or_else(|| sanitize_config_token(v));
                    if !sanitized.is_empty() {
                        if sanitized != v {
                            self.needs_repair = true;
                        }
                        self.mode = sanitized;
                    } else {
                        self.needs_repair = true;
                    }
                }
                "backend" if !v.is_empty() => {
                    self.backend = BackendKind::parse(v);
                }
                _ => {}
            }
        }
        if let Some(raw) = raw_effort {
            if let Some(clean) = self.backend.sanitize_effort(&raw) {
                if clean != raw {
                    self.needs_repair = true;
                }
                self.effort = clean;
            } else {
                self.needs_repair = true;
                self.effort = DEFAULT_EFFORT.to_string();
            }
        }
    }

    pub fn set_context_window(&mut self, model: &str, tokens: u64) {
        let key = sanitize_model_id(model);
        if key.is_empty() || tokens == 0 {
            return;
        }
        if let Some(entry) = self
            .context_windows
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&key))
        {
            entry.1 = tokens;
        } else {
            self.context_windows.push((key.to_string(), tokens));
        }
    }

    pub fn to_toml(&self) -> String {
        let model = sanitize_model_id(&self.model);
        let effort = self
            .backend
            .sanitize_effort(&self.effort)
            .unwrap_or_else(|| DEFAULT_EFFORT.to_string());
        let mode = BackendKind::normalize_mode(&self.mode)
            .map(str::to_string)
            .unwrap_or_else(|| sanitize_config_token(&self.mode));
        let mut payload = format!(
            "model = \"{}\"\neffort = \"{}\"\nmode = \"{}\"\nbackend = \"{}\"\n",
            model,
            effort,
            mode,
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

#[cfg(test)]
mod tests {
    use super::BackendKind;

    #[test]
    fn sanitize_effort_normalizes_valid_and_rejects_invalid_tokens() {
        use super::DEFAULT_EFFORT;
        for backend in [BackendKind::Agy, BackendKind::OpenCode] {
            assert_eq!(backend.sanitize_effort("low"), Some("low".to_string()));
            assert_eq!(backend.sanitize_effort(" HIGH "), Some("high".to_string()));
            assert_eq!(backend.sanitize_effort("xhigh!"), Some("xhigh".to_string()));
            assert_eq!(backend.sanitize_effort("Max"), Some("max".to_string()));
            assert_eq!(backend.sanitize_effort(""), None);
            assert_eq!(backend.sanitize_effort("minimal"), None);
            assert_eq!(backend.sanitize_effort("h/igh"), None);
            assert_eq!(backend.sanitize_effort("medium/bad"), None);
            assert_eq!(backend.sanitize_effort("!!!"), None);
        }

        let mut parsed = super::UserConfig::new_isolated();
        parsed.parse_content("effort = \"xhigh!\"\nbackend = \"opencode\"\n");
        assert_eq!(parsed.backend, BackendKind::OpenCode);
        assert_eq!(parsed.effort, "xhigh");
        assert!(parsed.needs_repair);

        let mut bad = super::UserConfig::new_isolated();
        bad.parse_content("effort = \"h/igh\"\nbackend = \"opencode\"\n");
        assert_eq!(bad.backend, BackendKind::OpenCode);
        assert_eq!(bad.effort, DEFAULT_EFFORT);
        assert!(bad.needs_repair);

        let mut dirty_mem = super::UserConfig::new_isolated();
        dirty_mem.backend = BackendKind::OpenCode;
        dirty_mem.effort = "h/igh".to_string();
        let rendered = dirty_mem.to_toml();
        assert!(rendered.contains(&format!("effort = \"{}\"\n", DEFAULT_EFFORT)));

        let mut dirty_valid = super::UserConfig::new_isolated();
        dirty_valid.backend = BackendKind::OpenCode;
        dirty_valid.effort = " XHIGH! ".to_string();
        let rendered_valid = dirty_valid.to_toml();
        assert!(rendered_valid.contains("effort = \"xhigh\"\n"));
    }
}
