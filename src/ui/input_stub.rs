#[derive(Clone, Debug, Default)]
pub struct MinimalInputStub {
    buffer: String,
    cursor_char_idx: usize,
}

impl MinimalInputStub {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor_char_idx: 0,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        let byte_pos = self.char_to_byte_index(self.cursor_char_idx);
        self.buffer.insert(byte_pos, ch);
        self.cursor_char_idx += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        let byte_pos = self.char_to_byte_index(self.cursor_char_idx);
        self.buffer.insert_str(byte_pos, s);
        self.cursor_char_idx += s.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor_char_idx > 0 {
            self.cursor_char_idx -= 1;
            let byte_pos = self.char_to_byte_index(self.cursor_char_idx);
            self.buffer.remove(byte_pos);
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor_char_idx = 0;
    }

    pub fn take_submitted(&mut self) -> Option<String> {
        let trimmed = self.buffer.trim();
        if trimmed.is_empty() {
            None
        } else {
            let result = self.buffer.clone();
            self.clear();
            Some(result)
        }
    }

    pub fn content(&self) -> &str {
        &self.buffer
    }

    pub fn cursor_char_idx(&self) -> usize {
        self.cursor_char_idx
    }

    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.buffer.len())
    }
}
