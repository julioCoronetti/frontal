use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem<T = String> {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub badge: Option<String>,
    pub value: T,
}

impl<T> SelectItem<T> {
    pub fn new(id: impl Into<String>, title: impl Into<String>, value: T) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category: None,
            description: None,
            shortcut: None,
            badge: None,
            value,
        }
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        self.badge = Some(badge.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectResult<T> {
    None,
    Cancel,
    Selected(SelectItem<T>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogSelect<T = String> {
    pub title: String,
    pub placeholder: String,
    pub query: String,
    pub items: Vec<SelectItem<T>>,
    pub selected_idx: usize,
    pub scroll_offset: Cell<usize>,
}

impl<T: Clone> DialogSelect<T> {
    pub fn new(title: impl Into<String>, items: Vec<SelectItem<T>>) -> Self {
        Self {
            title: title.into(),
            placeholder: String::new(),
            query: String::new(),
            items,
            selected_idx: 0,
            scroll_offset: Cell::new(0),
        }
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                if q.is_empty() {
                    true
                } else {
                    it.title.to_lowercase().contains(&q)
                        || it.id.to_lowercase().contains(&q)
                        || it
                            .category
                            .as_ref()
                            .map(|c| c.to_lowercase().contains(&q))
                            .unwrap_or(false)
                        || it
                            .description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&q))
                            .unwrap_or(false)
                        || it
                            .shortcut
                            .as_ref()
                            .map(|s| s.to_lowercase().contains(&q))
                            .unwrap_or(false)
                }
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn selected_item(&self) -> Option<&SelectItem<T>> {
        let filtered = self.filtered_indices();
        filtered.get(self.selected_idx).map(|&idx| &self.items[idx])
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> SelectResult<T> {
        let filtered = self.filtered_indices();
        match key.code {
            KeyCode::Esc => SelectResult::Cancel,
            KeyCode::Enter => {
                if let Some(&item_idx) = filtered.get(self.selected_idx) {
                    SelectResult::Selected(self.items[item_idx].clone())
                } else {
                    SelectResult::None
                }
            }
            KeyCode::Up | KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                SelectResult::None
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                SelectResult::None
            }
            KeyCode::Up => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                SelectResult::None
            }
            KeyCode::Down | KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !filtered.is_empty() && self.selected_idx + 1 < filtered.len() {
                    self.selected_idx += 1;
                }
                SelectResult::None
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !filtered.is_empty() && self.selected_idx + 1 < filtered.len() {
                    self.selected_idx += 1;
                }
                SelectResult::None
            }
            KeyCode::Down => {
                if !filtered.is_empty() && self.selected_idx + 1 < filtered.len() {
                    self.selected_idx += 1;
                }
                SelectResult::None
            }
            KeyCode::PageUp => {
                self.selected_idx = self.selected_idx.saturating_sub(5);
                SelectResult::None
            }
            KeyCode::PageDown => {
                if !filtered.is_empty() {
                    self.selected_idx = (self.selected_idx + 5).min(filtered.len() - 1);
                }
                SelectResult::None
            }
            KeyCode::Home => {
                self.selected_idx = 0;
                self.scroll_offset.set(0);
                SelectResult::None
            }
            KeyCode::End => {
                if !filtered.is_empty() {
                    self.selected_idx = filtered.len() - 1;
                }
                SelectResult::None
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.selected_idx = 0;
                self.scroll_offset.set(0);
                SelectResult::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_idx = 0;
                self.scroll_offset.set(0);
                SelectResult::None
            }
            _ => SelectResult::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if area.width < 4 || area.height < 4 {
            frame.render_widget(Clear, area);
            return;
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .style(Style::default().bg(Color::Reset));

        let filtered = self.filtered_indices();
        let mut lines = Vec::new();

        let title = format!(" {}:", self.title.trim_end_matches(':'));
        let esc_pad = (area.width as usize).saturating_sub(title.chars().count() + 5);
        lines.push(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:>esc_pad$}", "esc "),
                Style::default().fg(TEXT_MUTED),
            ),
        ]));
        lines.push(Line::raw(""));

        let display_query = if self.query.is_empty() && !self.placeholder.is_empty() {
            Span::styled(
                format!("Search: {}", self.placeholder),
                Style::default().fg(TEXT_MUTED),
            )
        } else if self.query.is_empty() {
            Span::styled("Search: ", Style::default().fg(TEXT_MUTED))
        } else {
            Span::styled(
                format!("Search: {}", self.query),
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )
        };

        lines.push(Line::from(vec![
            Span::styled(
                "   > ",
                Style::default()
                    .fg(ACCENT_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
            display_query,
            Span::styled("█", Style::default().fg(ACCENT_HIGHLIGHT)),
        ]));
        let line_w = (area.width as usize).saturating_sub(6);
        let divider = "─".repeat(line_w);
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", divider),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        lines.push(Line::raw(""));

        let mut rows = Vec::new();
        let mut last_category: Option<&str> = None;
        for (filtered_pos, &item_idx) in filtered.iter().enumerate() {
            let item = &self.items[item_idx];
            if let Some(ref cat) = item.category {
                if last_category != Some(cat.as_str()) {
                    rows.push((true, cat.clone(), filtered_pos));
                    last_category = Some(cat.as_str());
                }
            }
            rows.push((false, String::new(), filtered_pos));
        }

        let selected_row_idx = rows
            .iter()
            .position(|(is_cat, _, pos)| !*is_cat && *pos == self.selected_idx)
            .unwrap_or(0);

        let inner_height = area.height.saturating_sub(7) as usize;
        let mut offset = self.scroll_offset.get();
        if selected_row_idx < offset {
            offset = if selected_row_idx > 0 && rows[selected_row_idx - 1].0 {
                selected_row_idx - 1
            } else {
                selected_row_idx
            };
        } else if inner_height > 0 && selected_row_idx >= offset + inner_height {
            offset = selected_row_idx + 1 - inner_height;
        }
        self.scroll_offset.set(offset);

        let visible_slice = if inner_height > 0 && !rows.is_empty() {
            let end = (offset + inner_height).min(rows.len());
            &rows[offset..end]
        } else {
            &[]
        };

        for (is_cat, cat_name, pos) in visible_slice {
            if *is_cat {
                lines.push(Line::from(vec![Span::styled(
                    format!("  [{}]", cat_name),
                    Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
                )]));
            } else {
                let item = &self.items[filtered[*pos]];
                let is_sel = *pos == self.selected_idx;
                let (style, badge_style, shortcut_style, desc_style) = if is_sel {
                    (
                        Style::default()
                            .bg(ACCENT_RUBY)
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                        Style::default()
                            .bg(ACCENT_RUBY)
                            .fg(ACCENT_HIGHLIGHT)
                            .add_modifier(Modifier::BOLD),
                        Style::default().bg(ACCENT_RUBY).fg(TEXT_PRIMARY),
                        Style::default().bg(ACCENT_RUBY).fg(TEXT_MUTED),
                    )
                } else {
                    (
                        Style::default().fg(TEXT_PRIMARY),
                        Style::default().fg(ACCENT_HIGHLIGHT),
                        Style::default().fg(TEXT_MUTED),
                        Style::default().fg(TEXT_MUTED),
                    )
                };
                let prefix = if is_sel { " ● " } else { "   " };
                let inner_w = (area.width as usize).saturating_sub(2);

                let (display_title, title_chars) = if item.shortcut.is_some()
                    && item.badge.is_none()
                    && item.description.is_none()
                {
                    let shortcut_cols = item
                        .shortcut
                        .as_ref()
                        .map(|s| s.chars().count() + 2)
                        .unwrap_or(0);
                    let max_title_cols = inner_w.saturating_sub(3 + shortcut_cols);
                    let count = item.title.chars().count();
                    if count > max_title_cols && max_title_cols > 3 {
                        let take = max_title_cols - 3;
                        let mut truncated: String = item.title.chars().take(take).collect();
                        truncated.push_str("...");
                        let t_count = truncated.chars().count();
                        (truncated, t_count)
                    } else {
                        (item.title.clone(), count)
                    }
                } else {
                    let count = item.title.chars().count();
                    (item.title.clone(), count)
                };

                let mut spans = vec![
                    Span::styled(prefix, style),
                    Span::styled(display_title, style),
                ];
                let mut left_len = 3 + title_chars;
                if let Some(ref badge) = item.badge {
                    spans.push(Span::styled(format!(" [{}]", badge), badge_style));
                    left_len += badge.chars().count() + 3;
                }
                if let Some(ref desc) = item.description {
                    spans.push(Span::styled(format!(" - {}", desc), desc_style));
                    left_len += desc.chars().count() + 3;
                }
                if let Some(ref shortcut) = item.shortcut {
                    let shortcut_len = shortcut.chars().count() + 2;
                    let spaces = inner_w.saturating_sub(left_len + shortcut_len);
                    if spaces > 0 {
                        spans.push(Span::styled(" ".repeat(spaces), style));
                    }
                    spans.push(Span::styled(format!(" {} ", shortcut), shortcut_style));
                } else if is_sel && inner_w > left_len {
                    spans.push(Span::styled(" ".repeat(inner_w - left_len), style));
                }
                lines.push(Line::from(spans));
            }
        }

        if filtered.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "   (No matching results)",
                Style::default().fg(TEXT_MUTED),
            )]));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
