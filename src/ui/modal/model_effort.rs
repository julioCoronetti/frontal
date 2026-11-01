use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
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
}

pub struct ModelEffortModal {
    pub step: ModelEffortStep,
    pub query: String,
    pub models: Vec<ModelItem>,
    pub efforts: Vec<String>,
    pub selected_model_idx: usize,
    pub selected_effort_idx: usize,
    pub chosen_model: Option<String>,
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
        let items = models
            .into_iter()
            .map(|(id, name)| ModelItem { id, name })
            .collect();
        Self {
            step: ModelEffortStep::SelectModel,
            query: String::new(),
            models: items,
            efforts: vec![
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
                "max".to_string(),
            ],
            selected_model_idx: 0,
            selected_effort_idx: 2,
            chosen_model: None,
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
                KeyCode::Tab => {
                    let filtered = self.filtered_indices();
                    if let Some(&real_idx) = filtered.get(self.selected_model_idx) {
                        self.chosen_model = Some(self.models[real_idx].id.clone());
                    }
                    self.step = ModelEffortStep::SelectEffort;
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
                KeyCode::Enter => {
                    let filtered = self.filtered_indices();
                    if let Some(&real_idx) = filtered.get(self.selected_model_idx) {
                        let model = self.models[real_idx].id.clone();
                        let effort = if model.ends_with("-low") {
                            "low".to_string()
                        } else if model.ends_with("-medium") {
                            "medium".to_string()
                        } else if model.ends_with("-high") {
                            "high".to_string()
                        } else if model.ends_with("-xhigh") {
                            "xhigh".to_string()
                        } else if model.ends_with("-max") {
                            "max".to_string()
                        } else {
                            self.efforts[self.selected_effort_idx].clone()
                        };
                        self.chosen_model = Some(model.clone());
                        ModalAction::SelectModelEffort { model, effort }
                    } else {
                        ModalAction::None
                    }
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
                    if self.selected_effort_idx + 1 < self.efforts.len() {
                        self.selected_effort_idx += 1;
                    }
                    ModalAction::None
                }
                KeyCode::Tab => {
                    self.selected_effort_idx = (self.selected_effort_idx + 1) % self.efforts.len();
                    ModalAction::None
                }
                KeyCode::BackTab => {
                    self.selected_effort_idx =
                        (self.selected_effort_idx + self.efforts.len() - 1) % self.efforts.len();
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
                        .unwrap_or_else(|| "gemini-4-argon-high".to_string());
                    let effort = self.efforts[self.selected_effort_idx].clone();
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
                    .border_style(Style::default().fg(BORDER_SUBTLE))
                    .title(" Select Model (Tab: Effort, Enter: Confirm, Esc: Close) ");

                let mut lines = Vec::new();
                lines.push(Line::from(vec![
                    Span::styled(" Search: ", Style::default().fg(TEXT_MUTED)),
                    Span::styled(
                        &self.query,
                        Style::default()
                            .fg(TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("█", Style::default().fg(ACCENT_PRIMARY)),
                ]));
                lines.push(Line::raw(""));

                let filtered = self.filtered_indices();
                let visible_count = area.height.saturating_sub(5) as usize;
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
                        let style = if is_sel {
                            Style::default()
                                .bg(ACCENT_PRIMARY)
                                .fg(Color::Black)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(TEXT_PRIMARY)
                        };
                        let prefix = if is_sel { " > " } else { "   " };
                        let m = &self.models[real_idx];
                        lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(format!("{:<26} ", m.name), style),
                            Span::styled(
                                format!("({})", m.id),
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
                    .border_style(Style::default().fg(BORDER_SUBTLE))
                    .title(" Select Reasoning Effort (Enter: Confirm, Esc: Back) ");

                let mut lines = Vec::new();
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

                for (idx, eff) in self.efforts.iter().enumerate() {
                    let is_sel = idx == self.selected_effort_idx;
                    let style = if is_sel {
                        Style::default()
                            .bg(ACCENT_PRIMARY)
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
