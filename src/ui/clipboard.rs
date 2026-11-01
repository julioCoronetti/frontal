use arboard::Clipboard;

pub struct SystemClipboard {
    internal: Option<Clipboard>,
    emit_osc52: bool,
    isolated_buffer: Option<String>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            internal: Clipboard::new().ok(),
            emit_osc52: true,
            isolated_buffer: None,
        }
    }

    pub fn new_isolated() -> Self {
        Self {
            internal: None,
            emit_osc52: false,
            isolated_buffer: None,
        }
    }

    pub fn is_isolated(&self) -> bool {
        !self.emit_osc52
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
