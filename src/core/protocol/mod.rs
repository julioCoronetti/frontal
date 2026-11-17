pub mod acp;
mod agy;
mod events;
pub mod opencode;

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

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
}

impl PromptPayload {
    pub fn new(text: impl Into<String>, media: Vec<MediaItem>) -> Self {
        Self {
            text: text.into(),
            media,
        }
    }

    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            media: Vec::new(),
        }
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
        let mut parts = Vec::new();
        if !self.text.trim().is_empty() {
            parts.push(self.text.trim_end().to_string());
        }
        for item in &self.media {
            let path = media_uri_to_path(&item.uri);
            parts.push(format!("[Attached Media: {} ({})]", path, item.mime_type));
        }
        parts.join("\n")
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

pub fn parse_engine_line(line: &str) -> Result<Option<StreamEvent>, ProtocolError> {
    if OpenCodeStreamParser::matches(line) {
        OpenCodeStreamParser::parse_line(line)
    } else {
        parse_stream_line(line)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        if let Ok(content) = fs::read_to_string("/proc/sys/kernel/random/uuid") {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                return Self(trimmed.to_string());
            }
        }
        let fallback_path = Path::new("/dev/urandom");
        if let Ok(mut file) = fs::File::open(fallback_path) {
            let mut b = [0u8; 16];
            if file.read_exact(&mut b).is_ok() {
                b[6] = (b[6] & 0x0f) | 0x40;
                b[8] = (b[8] & 0x3f) | 0x80;
                let formatted = format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
                );
                return Self(formatted);
            }
        }
        Self("00000000-0000-4000-8000-000000000000".to_string())
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
