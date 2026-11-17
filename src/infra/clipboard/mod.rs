mod image_codec;
mod platform;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use arboard::Clipboard;

pub use image_codec::{adler32, crc32_ieee, decode_base64, decode_bmp_to_rgba, encode_rgba_png};
pub use platform::windows_path_to_wsl;

static TMP_MEDIA_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedMediaImage {
    pub path: PathBuf,
    pub mime_type: &'static str,
}

fn default_media_cache_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".frontal")
            .join("cache")
            .join("media")
    })
}

pub struct SystemClipboard {
    internal: Option<Clipboard>,
    emit_osc52: bool,
    isolated_buffer: Option<String>,
    isolated_image: Option<(u32, u32, Vec<u8>)>,
    media_cache_dir: Option<PathBuf>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            internal: Clipboard::new().ok(),
            emit_osc52: true,
            isolated_buffer: None,
            isolated_image: None,
            media_cache_dir: default_media_cache_dir(),
        }
    }

    pub fn new_isolated() -> Self {
        Self {
            internal: None,
            emit_osc52: false,
            isolated_buffer: None,
            isolated_image: None,
            media_cache_dir: None,
        }
    }

    pub fn new_isolated_with_dir(dir: PathBuf) -> Self {
        Self {
            internal: None,
            emit_osc52: false,
            isolated_buffer: None,
            isolated_image: None,
            media_cache_dir: Some(dir),
        }
    }

    pub fn set_media_cache_dir(&mut self, dir: PathBuf) {
        self.media_cache_dir = Some(dir);
    }

    pub fn set_isolated_image(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        self.isolated_image = Some((width, height, rgba));
    }

    pub fn clear_isolated_image(&mut self) {
        self.isolated_image = None;
    }

    pub fn is_isolated(&self) -> bool {
        !self.emit_osc52
    }

    pub fn resolve_media_cache_dir(&self) -> Option<PathBuf> {
        self.media_cache_dir.clone().or_else(|| {
            if self.is_isolated() {
                Some(std::env::temp_dir().join("frontal_isolated_media"))
            } else {
                default_media_cache_dir()
            }
        })
    }

    pub fn get_image(&mut self) -> Result<Option<SavedMediaImage>, String> {
        let cache_dir = self
            .resolve_media_cache_dir()
            .ok_or_else(|| "unable to resolve media cache directory".to_string())?;
        if self.is_isolated() {
            let Some((w, h, bytes)) = self.isolated_image.clone() else {
                return Ok(None);
            };
            return save_rgba_png_dedup(&cache_dir, w, h, &bytes).map(Some);
        }
        if let Some(ref mut cb) = self.internal {
            if let Ok(img) = cb.get_image() {
                let w = u32::try_from(img.width)
                    .map_err(|_| "clipboard image width overflow".to_string())?;
                let h = u32::try_from(img.height)
                    .map_err(|_| "clipboard image height overflow".to_string())?;
                return save_rgba_png_dedup(&cache_dir, w, h, img.bytes.as_ref()).map(Some);
            }
        }
        platform::read_system_clipboard_image(&cache_dir)
    }

    pub fn get_text(&mut self) -> Option<String> {
        if self.is_isolated() {
            self.isolated_buffer.clone()
        } else if let Some(ref mut cb) = self.internal {
            cb.get_text()
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(platform::read_system_clipboard_text)
        } else {
            platform::read_system_clipboard_text()
        }
    }

    pub fn set_text(&mut self, text: &str) -> Result<(), String> {
        if self.is_isolated() {
            self.isolated_buffer = Some(text.to_string());
            return Ok(());
        }
        let mut arboard_ok = false;
        if let Some(ref mut cb) = self.internal {
            arboard_ok = cb.set_text(text.to_string()).is_ok();
        }
        if !arboard_ok && self.emit_osc52 {
            let osc = Self::format_osc52(text);
            let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        Ok(())
    }

    pub fn format_osc52(text: &str) -> String {
        format!("\x1b]52;c;{}\x07", Self::encode_base64(text.as_bytes()))
    }

    pub fn encode_base64(data: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
            out.push(TABLE[((n >> 18) & 63) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((n >> 6) & 63) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

fn write_atomic_dedup<F>(
    cache_dir: &Path,
    file_name: String,
    mime_type: &'static str,
    build_bytes: F,
) -> Result<SavedMediaImage, String>
where
    F: FnOnce() -> Result<Vec<u8>, String>,
{
    let file_path = cache_dir.join(file_name);
    if file_path.is_file() {
        return Ok(SavedMediaImage {
            path: file_path,
            mime_type,
        });
    }
    let bytes = build_bytes()?;
    fs::create_dir_all(cache_dir)
        .map_err(|e| format!("failed to create media cache dir: {}", e))?;
    let seq = TMP_MEDIA_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = cache_dir.join(format!(".tmp_{}_{}.part", std::process::id(), seq));
    fs::write(&tmp_path, &bytes).map_err(|e| format!("failed to write cached image: {}", e))?;
    if let Err(err) = fs::rename(&tmp_path, &file_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("failed to finalize cached image: {}", err));
    }
    Ok(SavedMediaImage {
        path: file_path,
        mime_type,
    })
}

pub fn save_rgba_png_dedup(
    cache_dir: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<SavedMediaImage, String> {
    let hash = crc32_ieee(rgba);
    let file_name = format!("img_{}x{}_{:08x}.png", width, height, hash);
    write_atomic_dedup(cache_dir, file_name, "image/png", || {
        encode_rgba_png(width, height, rgba)
    })
}

pub fn save_encoded_media_dedup(
    cache_dir: &Path,
    bytes: &[u8],
    extension: &str,
    mime_type: &'static str,
) -> Result<SavedMediaImage, String> {
    if bytes.is_empty() {
        return Err("image payload is empty".to_string());
    }
    let hash = crc32_ieee(bytes);
    let file_name = format!("img_{:08x}_{}.{}", hash, bytes.len(), extension);
    write_atomic_dedup(cache_dir, file_name, mime_type, || Ok(bytes.to_vec()))
}

pub fn read_system_clipboard_media_or_text(
    cache_dir: Option<PathBuf>,
) -> Result<(Option<SavedMediaImage>, Option<String>), String> {
    let dir = cache_dir
        .or_else(default_media_cache_dir)
        .ok_or_else(|| "unable to resolve media cache directory".to_string())?;
    if let Some(saved) = platform::read_system_clipboard_image(&dir)? {
        return Ok((Some(saved), None));
    }
    Ok((None, platform::read_system_clipboard_text()))
}
