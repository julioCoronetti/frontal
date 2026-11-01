use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::modal::ModalAction;
use crate::ui::theme::{ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelEffortStep {
    SelectModel,
    SelectEffort,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
    pub supported_efforts: Vec<String>,
}

pub struct ModelEffortModal {
    pub step: ModelEffortStep,
    pub query: String,
    pub models: Vec<ModelItem>,
    pub fallback_efforts: Vec<String>,
    pub selected_model_idx: usize,
    pub selected_effort_idx: usize,
    pub chosen_model: Option<String>,
    pub current_model: Option<String>,
}

impl Default for ModelEffortModal {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelEffortModal {
    pub fn new() -> Self {
        Self::with_models(crate::transport::EngineConfig::default_fallback_models())
    }

    pub fn with_models(models: Vec<(String, String)>) -> Self {
        let items = Self::normalize_catalog(models);
        let fallback = vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ];
        let initial_effort_idx = items
            .first()
            .map(|m| Self::default_effort_for_model(&m.supported_efforts).0)
            .unwrap_or(0);
        Self {
            step: ModelEffortStep::SelectModel,
            query: String::new(),
            models: items,
            fallback_efforts: fallback,
            selected_model_idx: 0,
            selected_effort_idx: initial_effort_idx,
            chosen_model: None,
            current_model: None,
        }
    }

    fn parse_model_id_and_effort(raw_id: &str, raw_name: &str) -> (String, String, Option<String>) {
        let known_efforts = [
            ("-low", " (Low)", "low"),
            ("-medium", " (Medium)", "medium"),
            ("-high", " (High)", "high"),
            ("-xhigh", " (XHigh)", "xhigh"),
            ("-max", " (Max)", "max"),
        ];

        for &(id_sfx, name_sfx, effort) in &known_efforts {
            if let Some(base_id) = raw_id.strip_suffix(id_sfx) {
                let base_name = raw_name.strip_suffix(name_sfx).unwrap_or(raw_name);
                return (
                    base_id.to_string(),
                    base_name.to_string(),
                    Some(effort.to_string()),
                );
            }
        }

        (raw_id.to_string(), raw_name.to_string(), None)
    }

    pub fn normalize_catalog(raw_models: Vec<(String, String)>) -> Vec<ModelItem> {
        let default_efforts = vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ];

        let mut result: Vec<ModelItem> = Vec::new();

        for (raw_id, raw_name) in raw_models {
            let (base_id, base_name, effort_opt) =
                Self::parse_model_id_and_effort(&raw_id, &raw_name);
            if let Some(existing) = result.iter_mut().find(|m| m.id == base_id) {
                if let Some(eff) = effort_opt {
                    if !existing.supported_efforts.contains(&eff) {
                        existing.supported_efforts.push(eff);
                        Self::sort_efforts(&mut existing.supported_efforts);
                    }
                }
            } else {
                let supported = match effort_opt {
                    Some(eff) => vec![eff],
                    None => default_efforts.clone(),
                };
                result.push(ModelItem {
                    id: base_id,
                    name: base_name,
                    supported_efforts: supported,
                });
            }
        }

        result
    }

    fn sort_efforts(efforts: &mut [String]) {
        let order = ["low", "medium", "high", "xhigh", "max"];
        efforts.sort_by_key(|e| {
            order
                .iter()
                .position(|&o| o == e.as_str())
                .unwrap_or(order.len())
        });
    }

    pub fn current_supported_efforts(&self) -> &[String] {
        let filtered = self.filtered_indices();
        if let Some(&idx) = filtered.get(self.selected_model_idx) {
            return &self.models[idx].supported_efforts;
        }
        &self.fallback_efforts
    }

    pub fn default_effort_for_model(efforts: &[String]) -> (usize, String) {
        if let Some(pos) = efforts.iter().position(|e| e == "high") {
            (pos, "high".to_string())
        } else if let Some(pos) = efforts.iter().position(|e| e == "medium") {
            (pos, "medium".to_string())
        } else if let Some(first) = efforts.first() {
            (0, first.clone())
        } else {
            (0, "high".to_string())
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        self.models
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                if q.is_empty() {
                    true
                } else {
                    m.id.to_lowercase().contains(&q) || m.name.to_lowercase().contains(&q)
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn effort_description(effort: &str) -> &'static str {
        match effort {
            "low" => "Fastest response, minimal reasoning — best for straightforward tasks",
            "medium" => "Balanced speed and reasoning — recommended for most tasks",
            "high" => "Deepest reasoning for complex problems — slower but strongest",
            "xhigh" => "Extended reasoning budget for difficult coding challenges",
            "max" => "Maximum reasoning capability and exhaustive verification",
            _ => "Configured reasoning effort level",
        }
    }

    fn sync_effort_after_model_change(&mut self, prev_effort: Option<String>) {
        let effs = self.current_supported_efforts();
        if let Some(prev) = prev_effort {
            if let Some(pos) = effs.iter().position(|e| e == &prev) {
                self.selected_effort_idx = pos;
                return;
            }
        }
        let (def_idx, _) = Self::default_effort_for_model(effs);
        self.selected_effort_idx = def_idx;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        let max_model = self.filtered_indices().len().saturating_sub(1);
        let max_eff = self.current_supported_efforts().len().saturating_sub(1);

        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.selected_model_idx = self.selected_model_idx.saturating_sub(1);
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            KeyCode::Down => {
                if self.selected_model_idx < max_model {
                    let prev_eff = self
                        .current_supported_efforts()
                        .get(self.selected_effort_idx)
                        .cloned();
                    self.selected_model_idx += 1;
                    self.sync_effort_after_model_change(prev_eff);
                }
                ModalAction::None
            }
            KeyCode::Left => {
                self.selected_effort_idx = self.selected_effort_idx.saturating_sub(1);
                ModalAction::None
            }
            KeyCode::Right => {
                self.selected_effort_idx = (self.selected_effort_idx + 1).min(max_eff);
                ModalAction::None
            }
            KeyCode::Tab => {
                let effs = self.current_supported_efforts();
                if !effs.is_empty() {
                    self.selected_effort_idx = (self.selected_effort_idx + 1) % effs.len();
                }
                ModalAction::None
            }
            KeyCode::BackTab => {
                let effs = self.current_supported_efforts();
                if !effs.is_empty() {
                    self.selected_effort_idx =
                        (self.selected_effort_idx + effs.len() - 1) % effs.len();
                }
                ModalAction::None
            }
            KeyCode::Enter => {
                let filtered = self.filtered_indices();
                if let Some(&real_idx) = filtered.get(self.selected_model_idx) {
                    let model = self.models[real_idx].id.clone();
                    let efforts = self.current_supported_efforts();
                    let effort = efforts
                        .get(self.selected_effort_idx)
                        .cloned()
                        .unwrap_or_else(|| "high".to_string());
                    self.chosen_model = Some(model.clone());
                    ModalAction::SelectModelEffort { model, effort }
                } else {
                    ModalAction::None
                }
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.query.push(c);
                self.selected_model_idx = 0;
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            KeyCode::Backspace => {
                let prev_eff = self
                    .current_supported_efforts()
                    .get(self.selected_effort_idx)
                    .cloned();
                self.query.pop();
                self.selected_model_idx = 0;
                self.sync_effort_after_model_change(prev_eff);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE));

        let mut lines = Vec::new();
        let title = " Select Model:";
        let esc_pad = (area.width as usize).saturating_sub(title.len() + 5);
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

        let query_span = if self.query.is_empty() {
            Span::styled("   Search: ", Style::default().fg(TEXT_MUTED))
        } else {
            Span::styled(
                format!("   Search: {}", self.query),
                Style::default()
                    .fg(TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )
        };
        lines.push(Line::from(vec![
            query_span,
            Span::styled("█", Style::default().fg(ACCENT_PRIMARY)),
        ]));
        let line_w = (area.width as usize).saturating_sub(6);
        let divider = "─".repeat(line_w);
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", divider),
            Style::default().fg(BORDER_SUBTLE),
        )]));
        lines.push(Line::raw(""));

        let filtered = self.filtered_indices();
        let visible_count = area.height.saturating_sub(10) as usize;
        let visible_count = visible_count.max(1);
        let scroll_offset = if self.selected_model_idx >= visible_count {
            self.selected_model_idx + 1 - visible_count
        } else {
            0
        };

        if filtered.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "   No models matching query",
                Style::default().fg(TEXT_MUTED),
            )]));
        } else {
            for (display_idx, &real_idx) in filtered
                .iter()
                .enumerate()
                .skip(scroll_offset)
                .take(visible_count)
            {
                let is_sel = display_idx == self.selected_model_idx;
                let m = &self.models[real_idx];
                let is_current = self
                    .current_model
                    .as_ref()
                    .map(|cur| cur == &m.id || cur.starts_with(&format!("{}-", m.id)))
                    .unwrap_or(false);

                let bullet = if is_sel { " ● " } else { "   " };
                let current_label = if is_current { "  (current)" } else { "" };

                let style = if is_sel {
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_PRIMARY)
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        bullet,
                        Style::default().fg(if is_sel { ACCENT_PRIMARY } else { TEXT_MUTED }),
                    ),
                    Span::styled(&m.name, style),
                    Span::styled(current_label, Style::default().fg(TEXT_MUTED)),
                ]));
            }
        }

        lines.push(Line::raw(""));

        let efforts = self.current_supported_efforts();
        if !efforts.is_empty() {
            let seg_len = if efforts.len() <= 3 { 7 } else { 5 };
            let seg_str = "─".repeat(seg_len);
            let track_nodes_len = efforts.len().saturating_sub(1) * (seg_len + 1) + 1;
            let total_track_len = 1 + 2 + track_nodes_len + 2 + 1;
            let pad_left = (area.width as usize)
                .saturating_sub(2)
                .saturating_sub(total_track_len)
                / 2;

            let mut track_spans = Vec::new();
            if pad_left > 0 {
                track_spans.push(Span::raw(" ".repeat(pad_left)));
            }
            track_spans.push(Span::styled("◀  ", Style::default().fg(ACCENT_PRIMARY)));

            for (i, _) in efforts.iter().enumerate() {
                if i > 0 {
                    let seg_color = if i <= self.selected_effort_idx {
                        ACCENT_PRIMARY
                    } else {
                        BORDER_SUBTLE
                    };
                    track_spans.push(Span::styled(seg_str.clone(), Style::default().fg(seg_color)));
                }

                if i == self.selected_effort_idx {
                    track_spans.push(Span::styled(
                        "◎",
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else if i < self.selected_effort_idx {
                    track_spans.push(Span::styled("●", Style::default().fg(ACCENT_PRIMARY)));
                } else {
                    track_spans.push(Span::styled("●", Style::default().fg(TEXT_MUTED)));
                }
            }
            track_spans.push(Span::styled("  ▶", Style::default().fg(ACCENT_PRIMARY)));
            lines.push(Line::from(track_spans));

            let mut label_spans = Vec::new();
            let mut curr_col = 0;
            for (i, eff) in efforts.iter().enumerate() {
                let node_col = pad_left + 3 + i * (seg_len + 1);
                let target_col = node_col.saturating_sub(eff.len() / 2);
                let spaces = target_col.saturating_sub(curr_col);
                if spaces > 0 {
                    label_spans.push(Span::raw(" ".repeat(spaces)));
                }
                let label_style = if i == self.selected_effort_idx {
                    Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_MUTED)
                };
                label_spans.push(Span::styled(eff.clone(), label_style));
                curr_col = target_col + eff.len();
            }
            lines.push(Line::from(label_spans));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}
