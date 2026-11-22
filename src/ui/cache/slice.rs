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

        let has_margin = self.last_width > 4;
        let mut extracted = Vec::new();
        let mut current = 0usize;
        for block in &self.blocks {
            let Some(cached) = self.l2_cache.get(&block.id) else {
                continue;
            };
            if current > e_line {
                break;
            }
            let block_len = cached.lines.len();
            if current + block_len <= s_line {
                current += block_len;
                continue;
            }
            let local_start = s_line.saturating_sub(current);
            for (offset, line) in cached.lines[local_start..].iter().enumerate() {
                let l = current + local_start + offset;
                if l > e_line {
                    break;
                }
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                let trimmed = text.trim_end();
                let (stripped, prefix_len) = match &block.kind {
                    crate::ui::cache::BlockKind::Assistant => {
                        if has_margin {
                            if let Some(rest) = trimmed.strip_prefix("  ") {
                                (rest, 2)
                            } else {
                                (trimmed, 0)
                            }
                        } else {
                            (trimmed, 0)
                        }
                    }
                    crate::ui::cache::BlockKind::User => {
                        if let Some(rest) = trimmed.strip_prefix("▌ ") {
                            (rest, 2)
                        } else if let Some(rest) = trimmed.strip_prefix('▌') {
                            (rest, 1)
                        } else {
                            (trimmed, 0)
                        }
                    }
                    _ => {
                        if has_margin && trimmed == "  │" {
                            ("", 4)
                        } else if trimmed == "│" {
                            ("", 2)
                        } else if has_margin && trimmed == "  ● Error:" {
                            ("", 11)
                        } else if trimmed == "● Error:" {
                            ("", 9)
                        } else if has_margin {
                            if let Some(rest) = trimmed.strip_prefix("  │ ") {
                                (rest, 4)
                            } else if let Some(rest) = trimmed.strip_prefix("  ● Error: ") {
                                (rest, 11)
                            } else if let Some(rest) = trimmed.strip_prefix("  ") {
                                (rest, 2)
                            } else {
                                (trimmed, 0)
                            }
                        } else if let Some(rest) = trimmed.strip_prefix("│ ") {
                            (rest, 2)
                        } else if let Some(rest) = trimmed.strip_prefix("● Error: ") {
                            (rest, 9)
                        } else {
                            (trimmed, 0)
                        }
                    }
                };

                if matches!(
                    block.kind,
                    crate::ui::cache::BlockKind::Thought | crate::ui::cache::BlockKind::Tool { .. }
                ) && !trimmed.starts_with("  │")
                    && !trimmed.starts_with('│')
                    && is_structural_metadata(stripped)
                {
                    continue;
                }

                if l == s_line || l == e_line {
                    let chars: Vec<char> = stripped.chars().collect();
                    let sc = s_col.saturating_sub(prefix_len).min(chars.len());
                    let ec = if e_col <= prefix_len {
                        0
                    } else {
                        e_col.saturating_sub(prefix_len).min(chars.len())
                    };
                    let slice: String = if l == s_line && l == e_line {
                        if sc <= ec {
                            chars[sc..ec].iter().collect()
                        } else {
                            String::new()
                        }
                    } else if l == s_line {
                        chars[sc..].iter().collect()
                    } else {
                        chars[..ec].iter().collect()
                    };
                    extracted.push(slice);
                } else {
                    extracted.push(stripped.to_string());
                }
            }
            current += block_len;
        }

        while extracted.last().is_some_and(|s| s.trim().is_empty()) {
            extracted.pop();
        }
        let leading = extracted.iter().take_while(|s| s.trim().is_empty()).count();

        let mut sanitized = Vec::new();
        let mut prev_empty = false;
        for line in extracted.into_iter().skip(leading) {
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

fn is_structural_metadata(stripped: &str) -> bool {
    let check = stripped.trim_start();
    check.starts_with("+ Thought") || check.starts_with("- Thought") || check.starts_with("● ")
}
