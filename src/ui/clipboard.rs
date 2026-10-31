use arboard::Clipboard;

pub struct SystemClipboard {
    internal: Option<Clipboard>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            internal: Clipboard::new().ok(),
        }
    }

    pub fn get_text(&mut self) -> Option<String> {
        if let Some(ref mut cb) = self.internal {
            cb.get_text().ok()
        } else {
            None
        }
    }

    pub fn set_text(&mut self, text: &str) -> Result<(), String> {
        if let Some(ref mut cb) = self.internal {
            let _ = cb.set_text(text.to_string());
        }
        let osc = Self::format_osc52(text);
        let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
        let _ = std::io::Write::flush(&mut std::io::stdout());
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
