pub mod render;

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::Frame;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectItem<T = String> {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub badge: Option<String>,
    pub deletable: bool,
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
            deletable: false,
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

    pub fn with_deletable(mut self, deletable: bool) -> Self {
        self.deletable = deletable;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectResult<T> {
    None,
    Cancel,
    Selected(SelectItem<T>),
    Deleted(SelectItem<T>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogSelect<T = String> {
    pub title: String,
    pub placeholder: String,
    pub query: String,
    pub items: Vec<SelectItem<T>>,
    pub selected_idx: usize,
    pub pending_delete: Option<String>,
    pub scroll_offset: Cell<usize>,
    pub last_area: Cell<Rect>,
}

impl<T: Clone> DialogSelect<T> {
    pub fn new(title: impl Into<String>, items: Vec<SelectItem<T>>) -> Self {
        Self {
            title: title.into(),
            placeholder: String::new(),
            query: String::new(),
            items,
            selected_idx: 0,
            pending_delete: None,
            scroll_offset: Cell::new(0),
            last_area: Cell::new(Rect::default()),
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
                        || it
                            .badge
                            .as_ref()
                            .map(|b| b.to_lowercase().contains(&q))
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
        let is_delete_key = matches!(key.code, KeyCode::Delete)
            || matches!(
                key.code,
                KeyCode::Char('d' | 'D') if key.modifiers.contains(KeyModifiers::CONTROL)
            );
        if is_delete_key {
            return self.handle_delete_key(&filtered);
        }

        if key.code == KeyCode::Esc {
            if self.pending_delete.take().is_some() {
                return SelectResult::None;
            }
            return SelectResult::Cancel;
        }

        self.pending_delete = None;

        match key.code {
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
                    && !key.modifiers.contains(KeyModifiers::ALT)
                    && !c.is_control() =>
            {
                self.pending_delete = None;
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

    fn handle_delete_key(&mut self, filtered: &[usize]) -> SelectResult<T> {
        let Some(&item_idx) = filtered.get(self.selected_idx) else {
            self.pending_delete = None;
            return SelectResult::None;
        };
        if !self.items[item_idx].deletable {
            self.pending_delete = None;
            return SelectResult::None;
        }
        let item_id = self.items[item_idx].id.clone();
        if self.pending_delete.as_deref() == Some(item_id.as_str()) {
            self.pending_delete = None;
            let removed = self.items.remove(item_idx);
            let new_len = self.filtered_indices().len();
            if new_len == 0 {
                self.selected_idx = 0;
                self.scroll_offset.set(0);
            } else if self.selected_idx >= new_len {
                self.selected_idx = new_len - 1;
            }
            SelectResult::Deleted(removed)
        } else {
            self.pending_delete = Some(item_id);
            SelectResult::None
        }
    }

    pub(crate) fn build_visual_rows(&self, filtered: &[usize]) -> Vec<(bool, String, usize)> {
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
        rows
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> SelectResult<T> {
        let filtered = self.filtered_indices();
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.pending_delete = None;
                self.selected_idx = self.selected_idx.saturating_sub(1);
                SelectResult::None
            }
            MouseEventKind::ScrollDown => {
                self.pending_delete = None;
                if !filtered.is_empty() {
                    self.selected_idx = (self.selected_idx + 1).min(filtered.len() - 1);
                }
                SelectResult::None
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.pending_delete = None;
                let area = self.last_area.get();
                if area.width < 4 || area.height < 4 {
                    return SelectResult::None;
                }
                if mouse.column < area.x
                    || mouse.column >= area.x + area.width
                    || mouse.row < area.y
                    || mouse.row >= area.y + area.height
                {
                    return SelectResult::Cancel;
                }
                if mouse.row == area.y + 1 && mouse.column >= area.x + area.width.saturating_sub(6)
                {
                    return SelectResult::Cancel;
                }
                if mouse.column > area.x
                    && mouse.column < area.x + area.width - 1
                    && mouse.row >= area.y + 6
                    && mouse.row < area.y + area.height - 1
                {
                    let row_offset = (mouse.row - (area.y + 6)) as usize;
                    let row_idx = self.scroll_offset.get() + row_offset;
                    let rows = self.build_visual_rows(&filtered);
                    if let Some(&(false, _, filtered_pos)) = rows.get(row_idx) {
                        self.selected_idx = filtered_pos;
                        return SelectResult::Selected(self.items[filtered[filtered_pos]].clone());
                    }
                }
                SelectResult::None
            }
            _ => SelectResult::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        render::render_select(self, frame, area);
    }
}
