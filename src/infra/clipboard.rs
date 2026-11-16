use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use arboard::Clipboard;

const MAX_RAW_RGBA_BYTES: usize = 32 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
static MEDIA_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedMediaImage {
    pub path: PathBuf,
    pub mime_type: &'static str,
}

pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320u32 & mask);
        }
    }
    !crc
}

pub fn adler32(data: &[u8]) -> u32 {
    const MOD_ADLER: u32 = 65_521;
    let mut s1: u32 = 1;
    let mut s2: u32 = 0;
    for &byte in data {
        s1 = (s1 + byte as u32) % MOD_ADLER;
        s2 = (s2 + s1) % MOD_ADLER;
    }
    (s2 << 16) | s1
}

fn write_png_chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(chunk_type);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(chunk_type);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32_ieee(&crc_input).to_be_bytes());
}

pub fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("image dimensions must be non-zero".to_string());
    }
    let row_bytes = (width as usize)
        .checked_mul(4)
        .ok_or_else(|| "image row width overflow".to_string())?;
    let expected_len = row_bytes
        .checked_mul(height as usize)
        .ok_or_else(|| "image byte size overflow".to_string())?;
    if expected_len > MAX_RAW_RGBA_BYTES {
        return Err("image exceeds 32 MiB limit".to_string());
    }
    if rgba.len() != expected_len {
        return Err(format!(
            "rgba buffer length mismatch: expected {}, got {}",
            expected_len,
            rgba.len()
        ));
    }

    let mut filtered = Vec::with_capacity((row_bytes + 1) * (height as usize));
    for row in rgba.chunks_exact(row_bytes) {
        filtered.push(0);
        filtered.extend_from_slice(row);
    }

    let mut zlib = Vec::with_capacity(filtered.len() + 64);
    zlib.push(0x78);
    zlib.push(0x01);
    let total_chunks = filtered.len().div_ceil(65_535);
    for (idx, chunk) in filtered.chunks(65_535).enumerate() {
        let is_last = idx + 1 == total_chunks;
        zlib.push(u8::from(is_last));
        let len = chunk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&filtered).to_be_bytes());

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&width.to_be_bytes());
    ihdr[4..8].copy_from_slice(&height.to_be_bytes());
    ihdr[8] = 8;
    ihdr[9] = 6;

    let mut out = Vec::with_capacity(PNG_SIGNATURE.len() + 25 + zlib.len() + 24);
    out.extend_from_slice(&PNG_SIGNATURE);
    write_png_chunk(&mut out, b"IHDR", &ihdr);
    write_png_chunk(&mut out, b"IDAT", &zlib);
    write_png_chunk(&mut out, b"IEND", &[]);
    Ok(out)
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

    pub fn get_image(&mut self) -> Result<Option<SavedMediaImage>, String> {
        let (width, height, rgba) = if self.is_isolated() {
            let Some((w, h, bytes)) = self.isolated_image.clone() else {
                return Ok(None);
            };
            (w, h, bytes)
        } else if let Some(ref mut cb) = self.internal {
            match cb.get_image() {
                Ok(img) => {
                    let w = u32::try_from(img.width)
                        .map_err(|_| "clipboard image width overflow".to_string())?;
                    let h = u32::try_from(img.height)
                        .map_err(|_| "clipboard image height overflow".to_string())?;
                    (w, h, img.bytes.into_owned())
                }
                Err(_) => return Ok(None),
            }
        } else {
            return Ok(None);
        };

        let png_bytes = encode_rgba_png(width, height, &rgba)?;
        let cache_dir = self
            .media_cache_dir
            .clone()
            .or_else(|| {
                if self.is_isolated() {
                    Some(std::env::temp_dir().join("frontal_isolated_media"))
                } else {
                    default_media_cache_dir()
                }
            })
            .ok_or_else(|| "unable to resolve media cache directory".to_string())?;

        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("failed to create media cache dir: {}", e))?;

        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let hash = crc32_ieee(&rgba);
        let seq = MEDIA_SEQ.fetch_add(1, Ordering::Relaxed);
        let file_name = format!("img_{}_{:08x}_{}.png", timestamp_ms, hash, seq);
        let file_path = cache_dir.join(file_name);
        fs::write(&file_path, &png_bytes)
            .map_err(|e| format!("failed to write cached image: {}", e))?;

        Ok(Some(SavedMediaImage {
            path: file_path,
            mime_type: "image/png",
        }))
    }

    pub fn get_text(&mut self) -> Option<String> {
        if self.is_isolated() {
            self.isolated_buffer.clone()
        } else if let Some(ref mut cb) = self.internal {
            cb.get_text().ok()
        } else {
            None
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
