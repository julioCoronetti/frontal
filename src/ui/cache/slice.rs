use ratatui::text::Line;

use crate::ui::cache::RenderCache;

impl RenderCache {
    pub fn visible_slice(&self, offset: usize, count: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut current_idx = 0;
        let end_idx = offset.saturating_add(count);

        for block in &self.blocks {
            if let Some(cached) = self.l2_cache.get(&block.id) {
                for line in &cached.lines {
                    if current_idx >= offset && current_idx < end_idx {
                        lines.push(line.clone());
                    }
                    current_idx += 1;
                    if current_idx >= end_idx {
                        return lines;
                    }
                }
            }
        }

        lines
    }

    pub fn total_lines(&self) -> usize {
        let mut sum = 0;
        for block in &self.blocks {
            if let Some(cached) = self.l2_cache.get(&block.id) {
                sum += cached.lines.len();
            }
        }
        sum
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn cache_misses(&self) -> u64 {
        self.cache_misses
    }

    pub fn blocks_count(&self) -> usize {
        self.blocks.len()
    }

    pub fn line_info_at(&self, visual_idx: usize) -> Option<(crate::ui::cache::BlockKind, String)> {
        let mut current = 0;
        for block in &self.blocks {
            if let Some(cached) = self.l2_cache.get(&block.id) {
                if visual_idx < current + cached.lines.len() {
                    let local = visual_idx - current;
                    let line = &cached.lines[local];
                    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                    return Some((block.kind.clone(), text));
                }
                current += cached.lines.len();
            }
        }
        None
    }

    pub fn line_text_at(&self, visual_idx: usize) -> Option<String> {
        self.line_info_at(visual_idx).map(|(_, t)| t)
    }

    pub fn extract_text_range(
        &self,
        start_line: usize,
        start_col: usize,
        end_line: usize,
        end_col: usize,
    ) -> String {
        let (s_line, s_col, e_line, e_col) =
            if start_line > end_line || (start_line == end_line && start_col > end_col) {
                (end_line, end_col, start_line, start_col)
            } else {
                (start_line, start_col, end_line, end_col)
            };

        let mut extracted = Vec::new();
        for l in s_line..=e_line {
            if let Some((kind, text)) = self.line_info_at(l) {
                let trimmed = text.trim_end();
                let (stripped, prefix_len) = match kind {
                    crate::ui::cache::BlockKind::Assistant => (trimmed.to_string(), 0),
                    crate::ui::cache::BlockKind::User => {
                        if trimmed.starts_with("▌ ") {
                            (trimmed.replacen("▌ ", "", 1), 2)
                        } else if trimmed.starts_with('▌') {
                            (trimmed.replacen('▌', "", 1), 1)
                        } else {
                            (trimmed.to_string(), 0)
                        }
                    }
                    _ => {
                        if trimmed.starts_with("│ ") {
                            (trimmed.replacen("│ ", "", 1), 2)
                        } else if trimmed.starts_with("● Error: ") {
                            (trimmed.replacen("● Error: ", "", 1), 9)
                        } else if trimmed.starts_with("   ▎ ") {
                            (trimmed.replacen("   ▎ ", "", 1), 5)
                        } else if trimmed.starts_with("   ! ") {
                            (trimmed.replacen("   ! ", "", 1), 5)
                        } else if trimmed.starts_with("     ") {
                            (trimmed.replacen("     ", "", 1), 5)
                        } else if trimmed.starts_with("   ") {
                            (trimmed.replacen("   ", "", 1), 3)
                        } else {
                            (trimmed.to_string(), 0)
                        }
                    }
                };

                if is_structural_metadata(trimmed, &stripped) {
                    continue;
                }

                let chars: Vec<char> = stripped.chars().collect();
                let sc = s_col.saturating_sub(prefix_len).min(chars.len());
                let ec = if e_col <= prefix_len {
                    0
                } else {
                    e_col.saturating_sub(prefix_len).min(chars.len())
                };
                if l == s_line && l == e_line {
                    let slice: String = if sc <= ec {
                        chars[sc..ec].iter().collect()
                    } else {
                        String::new()
                    };
                    extracted.push(slice);
                } else if l == s_line {
                    let slice: String = chars[sc..].iter().collect();
                    extracted.push(slice);
                } else if l == e_line {
                    let slice: String = chars[..ec].iter().collect();
                    extracted.push(slice);
                } else {
                    extracted.push(stripped);
                }
            }
        }

        while extracted
            .first()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            extracted.remove(0);
        }
        while extracted
            .last()
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
        {
            extracted.pop();
        }

        let mut sanitized = Vec::new();
        let mut prev_empty = false;
        for line in extracted {
            let is_empty = line.trim().is_empty();
            if is_empty && prev_empty {
                continue;
            }
            prev_empty = is_empty;
            sanitized.push(line);
        }

        sanitized.join("\n")
    }
}

fn is_structural_metadata(trimmed: &str, stripped: &str) -> bool {
    trimmed.starts_with("▣ ")
        || stripped.starts_with("+ Thought")
        || stripped.starts_with("- Thought")
        || stripped.starts_with("▸ Thought")
        || stripped == "▾ Thought"
        || stripped.starts_with("● ")
        || stripped.starts_with("◌ ")
        || stripped.contains("[RUNNING]")
        || stripped.contains("[DONE]")
        || stripped.contains("[ERROR]")
        || stripped.contains("[PENDING]")
}
