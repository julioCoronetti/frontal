use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::diff::DiffViewer;
use crate::ui::theme::{ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModalAction {
    None,
    Close,
    SelectModelEffort {
        model: String,
        effort: String,
    },
    Respawn {
        conversation_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    },
    ExecuteCommand(String),
}

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
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                        && !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
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

pub struct HotRespawnModal {
    pub conversation_id: String,
    pub model: String,
    pub effort: String,
    pub mode: String,
    pub active_field: usize,
}

impl HotRespawnModal {
    pub fn new(
        conv_id: Option<String>,
        model: Option<String>,
        effort: Option<String>,
        mode: Option<String>,
    ) -> Self {
        Self {
            conversation_id: conv_id.unwrap_or_default(),
            model: model.unwrap_or_else(|| "gemini-4-argon-high".to_string()),
            effort: effort.unwrap_or_else(|| "high".to_string()),
            mode: mode.unwrap_or_else(|| "auto".to_string()),
            active_field: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Tab | KeyCode::Down => {
                self.active_field = (self.active_field + 1) % 4;
                ModalAction::None
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.active_field = (self.active_field + 3) % 4;
                ModalAction::None
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
            {
                let target = match self.active_field {
                    0 => &mut self.conversation_id,
                    1 => &mut self.model,
                    2 => &mut self.effort,
                    _ => &mut self.mode,
                };
                target.push(c);
                ModalAction::None
            }
            KeyCode::Backspace => {
                let target = match self.active_field {
                    0 => &mut self.conversation_id,
                    1 => &mut self.model,
                    2 => &mut self.effort,
                    _ => &mut self.mode,
                };
                target.pop();
                ModalAction::None
            }
            KeyCode::Enter => {
                let conv = if self.conversation_id.trim().is_empty() {
                    None
                } else {
                    Some(self.conversation_id.trim().to_string())
                };
                let mod_val = if self.model.trim().is_empty() {
                    None
                } else {
                    Some(self.model.trim().to_string())
                };
                let eff = if self.effort.trim().is_empty() {
                    None
                } else {
                    Some(self.effort.trim().to_string())
                };
                let md = if self.mode.trim().is_empty() {
                    None
                } else {
                    Some(self.mode.trim().to_string())
                };

                ModalAction::Respawn {
                    conversation_id: conv,
                    model: mod_val,
                    effort: eff,
                    mode: md,
                }
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .title(" Hot-Respawn Session (Tab/Shift+Tab: Navigate, Enter: Respawn, Esc: Cancel) ");

        let labels = [
            "Conversation UUID:",
            "Model ID:",
            "Reasoning Effort:",
            "Execution Mode:",
        ];
        let values = [&self.conversation_id, &self.model, &self.effort, &self.mode];

        let mut lines = Vec::new();
        lines.push(Line::raw(""));

        for i in 0..4 {
            let is_active = i == self.active_field;
            let field_style = if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let val_style = if is_active {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {:<20} ", labels[i]), field_style),
                Span::styled(format!("{}_", values[i]), val_style),
            ]));
            lines.push(Line::raw(""));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}

pub struct PaletteItem {
    pub id: String,
    pub title: String,
    pub shortcut: String,
}

pub struct CommandPaletteModal {
    pub query: String,
    pub items: Vec<PaletteItem>,
    pub selected_idx: usize,
}

impl Default for CommandPaletteModal {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPaletteModal {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            items: vec![
                PaletteItem {
                    id: "new_tab".to_string(),
                    title: "New Session Tab".to_string(),
                    shortcut: "Ctrl+T".to_string(),
                },
                PaletteItem {
                    id: "close_tab".to_string(),
                    title: "Close Session Tab".to_string(),
                    shortcut: "Ctrl+W".to_string(),
                },
                PaletteItem {
                    id: "select_model".to_string(),
                    title: "Select Model & Effort".to_string(),
                    shortcut: "/model".to_string(),
                },
                PaletteItem {
                    id: "hot_respawn".to_string(),
                    title: "Hot-Respawn Session".to_string(),
                    shortcut: "/respawn".to_string(),
                },
                PaletteItem {
                    id: "view_diff".to_string(),
                    title: "View Git/Patch Diff".to_string(),
                    shortcut: "/diff".to_string(),
                },
                PaletteItem {
                    id: "clear".to_string(),
                    title: "Clear Conversation Viewport".to_string(),
                    shortcut: "/clear".to_string(),
                },
                PaletteItem {
                    id: "interrupt".to_string(),
                    title: "Interrupt Running Turn".to_string(),
                    shortcut: "Ctrl+C".to_string(),
                },
                PaletteItem {
                    id: "quit".to_string(),
                    title: "Quit Frontal".to_string(),
                    shortcut: "Ctrl+Q".to_string(),
                },
            ],
            selected_idx: 0,
        }
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
                    it.title.to_lowercase().contains(&q) || it.shortcut.to_lowercase().contains(&q)
                }
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        let filtered = self.filtered_indices();
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                ModalAction::None
            }
            KeyCode::Down => {
                if !filtered.is_empty() && self.selected_idx + 1 < filtered.len() {
                    self.selected_idx += 1;
                }
                ModalAction::None
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && !key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.selected_idx = 0;
                ModalAction::None
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.selected_idx = 0;
                ModalAction::None
            }
            KeyCode::Enter => {
                if let Some(&item_idx) = filtered.get(self.selected_idx) {
                    let cmd_id = self.items[item_idx].id.clone();
                    ModalAction::ExecuteCommand(cmd_id)
                } else {
                    ModalAction::None
                }
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .title(" Command Palette (Enter: Run, Esc: Close) ");

        let filtered = self.filtered_indices();
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(
                " > ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}_", self.query),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::raw(""));

        for (pos, &idx) in filtered.iter().enumerate() {
            let item = &self.items[idx];
            let is_sel = pos == self.selected_idx;
            let style = if is_sel {
                Style::default()
                    .bg(Color::Magenta)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if is_sel { " * " } else { "   " };
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(format!("{:<30}", item.title), style),
                Span::styled(item.shortcut.clone(), Style::default().fg(Color::DarkGray)),
            ]));
        }

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}

pub struct DiffModal {
    pub viewer: DiffViewer,
}

impl DiffModal {
    pub fn new(diff_text: &str) -> Self {
        Self {
            viewer: DiffViewer::new(diff_text),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match key.code {
            KeyCode::Esc => ModalAction::Close,
            KeyCode::Up => {
                self.viewer.scroll_up(1);
                ModalAction::None
            }
            KeyCode::Down => {
                self.viewer.scroll_down(1);
                ModalAction::None
            }
            KeyCode::PageUp => {
                self.viewer.scroll_up(10);
                ModalAction::None
            }
            KeyCode::PageDown => {
                self.viewer.scroll_down(10);
                ModalAction::None
            }
            _ => ModalAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let total = self.viewer.line_count();
        let offset = self.viewer.scroll_offset;
        let title = format!(
            " Integrated Diff Viewer [Line {}/{} total] (Up/Down/PgUp/PgDn: Scroll, Esc: Close) ",
            if total == 0 { 0 } else { offset + 1 },
            total
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_SUBTLE))
            .title(title);

        let inner_height = area.height.saturating_sub(2) as usize;
        let lines = self.viewer.render_lines(inner_height);

        let widget = Paragraph::new(lines).block(block);
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
    }
}

pub enum Modal {
    ModelEffort(ModelEffortModal),
    HotRespawn(HotRespawnModal),
    CommandPalette(CommandPaletteModal),
    Diff(DiffModal),
}

impl Modal {
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        match self {
            Self::ModelEffort(m) => m.handle_key(key),
            Self::HotRespawn(m) => m.handle_key(key),
            Self::CommandPalette(m) => m.handle_key(key),
            Self::Diff(m) => m.handle_key(key),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        match self {
            Self::ModelEffort(m) => m.render(frame, area),
            Self::HotRespawn(m) => m.render(frame, area),
            Self::CommandPalette(m) => m.render(frame, area),
            Self::Diff(m) => m.render(frame, area),
        }
    }
}

pub struct ModalStack {
    pub stack: Vec<Modal>,
}

impl ModalStack {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    pub fn push(&mut self, modal: Modal) {
        self.stack.push(modal);
    }

    pub fn pop(&mut self) -> Option<Modal> {
        self.stack.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    pub fn top_mut(&mut self) -> Option<&mut Modal> {
        self.stack.last_mut()
    }

    pub fn top(&self) -> Option<&Modal> {
        self.stack.last()
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> ModalAction {
        if let Some(top) = self.stack.last_mut() {
            let act = top.handle_key(key);
            if act == ModalAction::Close {
                self.stack.pop();
            }
            act
        } else {
            ModalAction::None
        }
    }

    pub fn render(&self, frame: &mut Frame, screen_area: Rect) {
        if let Some(top) = self.stack.last() {
            let popup_area = Self::centered_rect(75, 60, screen_area);
            top.render(frame, popup_area);
        }
    }

    fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(r);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }
}

impl Default for ModalStack {
    fn default() -> Self {
        Self::new()
    }
}
