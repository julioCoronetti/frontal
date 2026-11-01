use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::modal::ModalAction;
use crate::ui::theme::{ACCENT_WARM, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

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
        Self {
            step: ModelEffortStep::SelectModel,
            query: String::new(),
            models: items,
            fallback_efforts: fallback,
            selected_model_idx: 0,
            selected_effort_idx: 0,
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
        let model_id = self.chosen_model.as_ref().or_else(|| {
            let filtered = self.filtered_indices();
            filtered
                .get(self.selected_model_idx)
                .map(|&idx| &self.models[idx].id)
        });
        if let Some(id) = model_id {
            if let Some(m) = self.models.iter().find(|m| &m.id == id) {
                return &m.supported_efforts;
            }
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

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match self.step {
            ModelEffortStep::SelectModel => match key.code {
                KeyCode::Esc => ModalAction::Close,
                KeyCode::Enter | KeyCode::Tab => {
                    let filtered = self.filtered_indices();
                    if let Some(&real_idx) = filtered.get(self.selected_model_idx) {
                        let m = &self.models[real_idx];
                        let (def_idx, _) = Self::default_effort_for_model(&m.supported_efforts);
                        self.chosen_model = Some(m.id.clone());
                        self.selected_effort_idx = def_idx;
                        self.step = ModelEffortStep::SelectEffort;
                    }
                    ModalAction::None
                }
                KeyCode::Up => {
                    self.selected_model_idx = self.selected_model_idx.saturating_sub(1);
                    ModalAction::None
                }
                KeyCode::Down => {
                    let filtered = self.filtered_indices();
                    if !filtered.is_empty() && self.selected_model_idx + 1 < filtered.len() {
                        self.selected_model_idx += 1;
                    }
                    ModalAction::None
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.query.push(c);
                    self.selected_model_idx = 0;
                    ModalAction::None
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.selected_model_idx = 0;
                    ModalAction::None
                }
                _ => ModalAction::None,
            },
            ModelEffortStep::SelectEffort => match key.code {
                KeyCode::Esc | KeyCode::Backspace => {
                    self.step = ModelEffortStep::SelectModel;
                    ModalAction::None
                }
                KeyCode::Up => {
                    self.selected_effort_idx = self.selected_effort_idx.saturating_sub(1);
                    ModalAction::None
                }
                KeyCode::Down => {
                    let efforts = self.current_supported_efforts();
                    if !efforts.is_empty() && self.selected_effort_idx + 1 < efforts.len() {
                        self.selected_effort_idx += 1;
                    }
                    ModalAction::None
                }
                KeyCode::Tab => {
                    let efforts = self.current_supported_efforts();
                    if !efforts.is_empty() {
                        self.selected_effort_idx = (self.selected_effort_idx + 1) % efforts.len();
                    }
                    ModalAction::None
                }
                KeyCode::BackTab => {
                    let efforts = self.current_supported_efforts();
                    if !efforts.is_empty() {
                        self.selected_effort_idx =
                            (self.selected_effort_idx + efforts.len() - 1) % efforts.len();
                    }
                    ModalAction::None
                }
                KeyCode::Enter => {
                    let model = self
                        .chosen_model
                        .clone()
                        .or_else(|| {
                            let filtered = self.filtered_indices();
                            filtered
                                .get(self.selected_model_idx)
                                .map(|&idx| self.models[idx].id.clone())
                        })
                        .unwrap_or_else(|| crate::config::DEFAULT_MODEL.to_string());
                    let efforts = self.current_supported_efforts();
                    let effort = efforts
                        .get(self.selected_effort_idx)
                        .cloned()
                        .unwrap_or_else(|| "high".to_string());
                    ModalAction::SelectModelEffort { model, effort }
                }
                _ => ModalAction::None,
            },
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self.step {
            ModelEffortStep::SelectModel => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_SUBTLE));

                let mut lines = Vec::new();
                let esc_pad = (area.width as usize).saturating_sub(18);
                lines.push(Line::from(vec![
                    Span::styled(
                        " Select model",
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:>esc_pad$}", "esc"),
                        Style::default().fg(TEXT_MUTED),
                    ),
                ]));
                lines.push(Line::raw(""));

                let query_span = if self.query.is_empty() {
                    Span::styled(" Search", Style::default().fg(TEXT_MUTED))
                } else {
                    Span::styled(
                        format!(" {}", self.query),
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    )
                };
                lines.push(Line::from(vec![
                    query_span,
                    Span::styled("█", Style::default().fg(ACCENT_WARM)),
                ]));
                lines.push(Line::raw(""));

                let filtered = self.filtered_indices();
                let visible_count = area.height.saturating_sub(6) as usize;
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

                        let dot_span = if is_current {
                            Span::styled("● ", Style::default().fg(ACCENT_WARM))
                        } else {
                            Span::raw("  ")
                        };

                        let style = if is_sel {
                            Style::default()
                                .bg(ACCENT_WARM)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(TEXT_PRIMARY)
                        };

                        let efforts_label = m.supported_efforts.join("·");
                        let total_w = area.width.saturating_sub(6) as usize;
                        let name_w = total_w.saturating_sub(efforts_label.len() + 1).max(10);

                        lines.push(Line::from(vec![
                            if is_sel {
                                Span::styled(" ", style)
                            } else {
                                dot_span
                            },
                            Span::styled(format!("{:<name_w$}", m.name), style),
                            Span::styled(
                                format!(" {}", efforts_label),
                                if is_sel {
                                    style
                                } else {
                                    Style::default().fg(TEXT_MUTED)
                                },
                            ),
                        ]));
                    }
                }

                let widget = Paragraph::new(lines).block(block);
                frame.render_widget(Clear, area);
                frame.render_widget(widget, area);
            }
            ModelEffortStep::SelectEffort => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(BORDER_SUBTLE));

                let mut lines = Vec::new();
                let esc_pad = (area.width as usize).saturating_sub(19);
                lines.push(Line::from(vec![
                    Span::styled(
                        " Select effort",
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:>esc_pad$}", "esc"),
                        Style::default().fg(TEXT_MUTED),
                    ),
                ]));
                lines.push(Line::raw(""));

                let chosen = self.chosen_model.as_deref().unwrap_or("None");
                lines.push(Line::from(vec![
                    Span::styled(" Model: ", Style::default().fg(TEXT_MUTED)),
                    Span::styled(
                        chosen,
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::raw(""));

                let efforts = self.current_supported_efforts();
                for (idx, eff) in efforts.iter().enumerate() {
                    let is_sel = idx == self.selected_effort_idx;
                    let style = if is_sel {
                        Style::default()
                            .bg(ACCENT_WARM)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(TEXT_PRIMARY)
                    };
                    let prefix = if is_sel { " > " } else { "   " };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, style),
                        Span::styled(format!("{:<16}", eff), style),
                    ]));
                }

                let widget = Paragraph::new(lines).block(block);
                frame.render_widget(Clear, area);
                frame.render_widget(widget, area);
            }
        }
    }
}
