use std::path::{Path, PathBuf};

use super::protocol::media_uri_to_path;

pub fn image_mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("png") {
        Some("image/png")
    } else if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") {
        Some("image/jpeg")
    } else if ext.eq_ignore_ascii_case("webp") {
        Some("image/webp")
    } else if ext.eq_ignore_ascii_case("gif") {
        Some("image/gif")
    } else if ext.eq_ignore_ascii_case("bmp") {
        Some("image/bmp")
    } else {
        None
    }
}

fn unescape_shell_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next) = chars.peek() {
                if matches!(next, ' ' | '\'' | '"' | '\\' | '(' | ')' | '[' | ']') {
                    chars.next();
                    out.push(next);
                    continue;
                }
            }
        }
        out.push(ch);
    }
    out
}

pub fn extract_pasted_image_path(raw: &str) -> Option<(PathBuf, &'static str)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('\r') {
        return None;
    }
    let unquoted = if (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2)
        || (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2)
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let normalized = if unquoted.starts_with("file://") {
        media_uri_to_path(unquoted)
    } else {
        unescape_shell_path(unquoted)
    };
    let path = PathBuf::from(normalized);
    let mime = image_mime_for_path(&path)?;
    if path.is_file() {
        Some((path, mime))
    } else {
        None
    }
}
