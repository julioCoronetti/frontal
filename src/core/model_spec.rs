use serde::{Deserialize, Serialize};

pub const COMPACTION_THRESHOLD_PCT: f64 = 75.0;

const FAMILY_CONTEXT_WINDOWS: &[(&str, u64)] = &[("gemini", 1_000_000), ("claude", 200_000)];
const EFFORT_SUFFIXES: &[&str] = &["-xhigh", "-medium", "-high", "-low", "-max"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextWindowSource {
    Protocol,
    Config,
    Heuristic,
}

impl ContextWindowSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Protocol => "protocol",
            Self::Config => "config",
            Self::Heuristic => "family",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextWindow {
    pub tokens: u64,
    pub source: ContextWindowSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelSpec {
    pub context_window: Option<ContextWindow>,
}

impl ModelSpec {
    pub fn resolve(
        models: &[&str],
        protocol_hint: Option<u64>,
        overrides: &[(String, u64)],
    ) -> Self {
        if let Some(tokens) = protocol_hint.filter(|tokens| *tokens > 0) {
            return Self::with(tokens, ContextWindowSource::Protocol);
        }
        let candidates: Vec<Vec<String>> =
            models.iter().map(|model| model_keys(&[model])).collect();
        for keys in &candidates {
            if keys.is_empty() {
                continue;
            }
            for (name, tokens) in overrides {
                if *tokens == 0 {
                    continue;
                }
                let override_keys = model_keys(&[name.as_str()]);
                if override_keys
                    .iter()
                    .any(|candidate| keys.iter().any(|key| key == candidate))
                {
                    return Self::with(*tokens, ContextWindowSource::Config);
                }
            }
        }
        for raw in models {
            let slug = bare_slug(raw);
            if slug.is_empty() {
                continue;
            }
            for (family, tokens) in FAMILY_CONTEXT_WINDOWS {
                if slug.contains(family) {
                    return Self::with(*tokens, ContextWindowSource::Heuristic);
                }
            }
        }
        Self::default()
    }

    fn with(tokens: u64, source: ContextWindowSource) -> Self {
        Self {
            context_window: Some(ContextWindow { tokens, source }),
        }
    }

    pub fn usable(&self) -> Option<u64> {
        self.context_window
            .map(|window| window.tokens)
            .filter(|tokens| *tokens > 0)
    }

    pub fn occupancy_pct(&self, used: u64) -> Option<f64> {
        self.usable()
            .map(|tokens| (used as f64 / tokens as f64 * 100.0).clamp(0.0, 100.0))
    }

    pub fn remaining(&self, used: u64) -> Option<u64> {
        self.usable().map(|tokens| tokens.saturating_sub(used))
    }

    pub fn compaction_threshold(&self) -> Option<u64> {
        self.usable()
            .map(|tokens| (tokens as f64 * COMPACTION_THRESHOLD_PCT / 100.0) as u64)
    }

    pub fn compact_margin(&self, used: u64) -> Option<u64> {
        self.compaction_threshold()
            .map(|threshold| threshold.saturating_sub(used))
    }

    pub fn is_compaction_recommended(&self, used: u64) -> bool {
        self.occupancy_pct(used)
            .map(|pct| pct >= COMPACTION_THRESHOLD_PCT)
            .unwrap_or(false)
    }
}

fn model_keys(models: &[&str]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for raw in models {
        let normalized = normalize_model_id(raw);
        if normalized.is_empty() {
            continue;
        }
        for candidate in model_variants(&normalized) {
            if !candidate.is_empty() && !keys.iter().any(|existing| existing == &candidate) {
                keys.push(candidate);
            }
        }
    }
    keys
}

fn bare_slug(raw: &str) -> String {
    let normalized = normalize_model_id(raw);
    normalized
        .rsplit_once('/')
        .map(|(_, bare)| bare.to_string())
        .unwrap_or(normalized)
}

fn model_variants(normalized: &str) -> Vec<String> {
    let mut bases = vec![normalized.to_string()];
    if let Some((_, bare)) = normalized.rsplit_once('/') {
        bases.push(bare.to_string());
    }
    let mut variants = Vec::new();
    for base in bases {
        variants.push(base.clone());
        variants.push(strip_effort(&base));
    }
    variants
}

fn normalize_model_id(raw: &str) -> String {
    raw.trim()
        .to_lowercase()
        .split('#')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn strip_effort(model: &str) -> String {
    for suffix in EFFORT_SUFFIXES {
        if let Some(stripped) = model.strip_suffix(suffix) {
            return stripped.to_string();
        }
    }
    model.to_string()
}
