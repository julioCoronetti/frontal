pub mod acp;
mod agy;
mod events;
pub mod opencode;

use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);

pub use agy::{format_user_message, format_user_payload, parse_stream_line, AgyStreamParser};
pub use events::*;
pub use opencode::{extract_opencode_session_id, fnv1a_64, OpenCodeStreamParser};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaItem {
    pub mime_type: String,
    pub uri: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptPayload {
    pub text: String,
    #[serde(default)]
    pub media: Vec<MediaItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_text: Option<String>,
}

impl PromptPayload {
    pub fn new(text: impl Into<String>, media: Vec<MediaItem>) -> Self {
        Self {
            text: text.into(),
            media,
            chat_text: None,
        }
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            media: Vec::new(),
            chat_text: None,
        }
    }

    pub fn with_chat_text(mut self, chat_text: impl Into<String>) -> Self {
        self.chat_text = Some(chat_text.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.media.is_empty()
    }

    pub fn summary_text(&self) -> String {
        if !self.text.trim().is_empty() {
            self.text.clone()
        } else if self.media.is_empty() {
            String::new()
        } else {
            format!("[{} attached image(s)]", self.media.len())
        }
    }

    pub fn display_for_chat(&self) -> String {
        if self.media.is_empty() {
            return self.text.clone();
        }
        let body = match &self.chat_text {
            Some(chat) if !chat.trim().is_empty() => chat.trim_end().to_string(),
            _ => {
                let markers: Vec<String> = (1..=self.media.len())
                    .map(|idx| format!("[Image {}]", idx))
                    .collect();
                if self.text.trim().is_empty() {
                    markers.join(" ")
                } else {
                    format!("{} {}", self.text.trim_end(), markers.join(" "))
                }
            }
        };
        let footer: Vec<String> = self
            .media
            .iter()
            .map(|item| format!("[file] {}", media_source_label(&item.uri)))
            .collect();
        format!("{}\n\n{}", body, footer.join("\n"))
    }
}

impl From<&str> for PromptPayload {
    fn from(value: &str) -> Self {
        Self::from_text(value)
    }
}

impl From<String> for PromptPayload {
    fn from(value: String) -> Self {
        Self::from_text(value)
    }
}

impl From<&String> for PromptPayload {
    fn from(value: &String) -> Self {
        Self::from_text(value.as_str())
    }
}

pub fn percent_decode_utf8(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn media_uri_to_path(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("file://localhost") {
        percent_decode_utf8(rest)
    } else if let Some(rest) = uri.strip_prefix("file://") {
        percent_decode_utf8(rest)
    } else {
        uri.to_string()
    }
}

pub fn media_source_label(uri: &str) -> String {
    let decoded = media_uri_to_path(uri);
    let path = Path::new(&decoded);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty());
    let is_clipboard_cache = file_name.is_some_and(|name| name.starts_with("img_"))
        && path.parent().is_some_and(|parent| {
            let dir_name = parent.file_name().and_then(|n| n.to_str());
            let grandparent_name = parent
                .parent()
                .and_then(|gp| gp.file_name())
                .and_then(|n| n.to_str());
            dir_name == Some("frontal_isolated_media")
                || (dir_name == Some("media") && grandparent_name == Some("cache"))
        });
    if is_clipboard_cache {
        return String::from("clipboard");
    }
    file_name
        .map(ToString::to_string)
        .unwrap_or_else(|| decoded)
}

pub fn parse_engine_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.starts_with("error:") {
        return AgyStreamParser::parse_line(trimmed);
    }
    let val: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| ProtocolError::Deserialization(e.to_string()))?;
    if OpenCodeStreamParser::matches_value(&val) {
        Ok(OpenCodeStreamParser::parse_value(&val))
    } else {
        Ok(Some(AgyStreamParser::parse_value(val)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        let seq = NEXT_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let h1 = RandomState::new().build_hasher().finish() ^ nanos;
        let h2 = RandomState::new().build_hasher().finish().wrapping_add(seq);
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&h1.to_le_bytes());
        b[8..].copy_from_slice(&h2.to_le_bytes());
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        let formatted = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        );
        Self(formatted)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProtocolError {
    EmptyPrompt,
    UnsupportedMedia(String),
    Serialization(String),
    Deserialization(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPrompt => write!(f, "Prompt content cannot be empty"),
            Self::UnsupportedMedia(err) => write!(f, "Unsupported media: {}", err),
            Self::Serialization(err) => write!(f, "Serialization error: {}", err),
            Self::Deserialization(err) => write!(f, "Deserialization error: {}", err),
        }
    }
}

impl std::error::Error for ProtocolError {}
