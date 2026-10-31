use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::protocol::{EngineEvent, SessionEnvelope, SessionId, StepState, StreamEvent};
use crate::session::{SessionManager, SessionStatus};
use crate::ui::autocomplete::{AutocompletePopover, FileSearchWorker};
use crate::ui::cache::{BlockKind, RenderCache};
use crate::ui::clipboard::SystemClipboard;
use crate::ui::editor::{EditorSnapshot, ExtmarkKind, PromptEditor, TriggerKind};
use crate::ui::history::PromptHistory;
use crate::ui::input_stub::MinimalInputStub;
use crate::ui::viewport::{ConversationViewport, ScrollMode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppAction {
    SubmitPrompt(String),
    Interrupt,
    Quit,
}

pub struct App {
    pub session_manager: SessionManager,
    pub active_session: SessionId,
    pub cache: RenderCache,
    pub viewport: ConversationViewport,
    pub input: MinimalInputStub,
    pub editor: PromptEditor,
    pub popover: AutocompletePopover,
    pub search_worker: FileSearchWorker,
    pub history: PromptHistory,
    pub draft_snapshot: Option<EditorSnapshot>,
    pub dirty: bool,
    pub should_quit: bool,
    pub status_text: String,
    pub spinner_tick: u64,
    pub waiting_for_response: bool,
}

impl App {
    pub fn new(session_manager: SessionManager, active_session: SessionId) -> Self {
        let history = if let Ok(home) = std::env::var("HOME") {
            let path = std::path::PathBuf::from(home)
                .join(".frontal")
                .join("history.db");
            PromptHistory::open(&path).unwrap_or_else(|_| PromptHistory::open_in_memory().unwrap())
        } else {
            PromptHistory::open_in_memory().unwrap()
        };

        let search_worker = FileSearchWorker::new();
        if let Ok(cwd) = std::env::current_dir() {
            search_worker.start_background_scan(cwd, 500);
        }

        Self {
            session_manager,
            active_session,
            cache: RenderCache::new(),
            viewport: ConversationViewport::new(),
            input: MinimalInputStub::new(),
            editor: PromptEditor::new(),
            popover: AutocompletePopover::new(),
            search_worker,
            history,
            draft_snapshot: None,
            dirty: true,
            should_quit: false,
            status_text: String::from("Ready"),
            spinner_tick: 0,
            waiting_for_response: false,
        }
    }

    pub fn handle_terminal_event(&mut self, event: Event) -> Option<AppAction> {
        match event {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    return None;
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    let is_running = self
                        .session_manager
                        .get_metadata(&self.active_session)
                        .map(|m| m.status == SessionStatus::RunningTurn)
                        .unwrap_or(false);
                    if is_running {
                        let _ = self.session_manager.interrupt_active();
                        self.status_text = String::from("Interrupted by user");
                        self.waiting_for_response = false;
                        self.dirty = true;
                        return Some(AppAction::Interrupt);
                    } else {
                        self.should_quit = true;
                        return Some(AppAction::Quit);
                    }
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('v') {
                    let mut cb = SystemClipboard::new();
                    if let Some(text) = cb.get_text() {
                        self.editor.insert_paste(&text);
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                        return None;
                    }
                }

                if self.popover.is_visible() {
                    match key.code {
                        KeyCode::Up => {
                            self.popover.select_prev();
                            self.dirty = true;
                            return None;
                        }
                        KeyCode::Down => {
                            self.popover.select_next();
                            self.dirty = true;
                            return None;
                        }
                        KeyCode::Esc => {
                            self.popover.close();
                            self.dirty = true;
                            return None;
                        }
                        KeyCode::Tab | KeyCode::Enter => {
                            if let Some(item) = self.popover.selected_item().cloned() {
                                let start = self.popover.trigger_start_char;
                                let end = self.editor.cursor_char();
                                let kind = match item.kind {
                                    TriggerKind::File => ExtmarkKind::FileMention {
                                        path: item.label.clone(),
                                    },
                                    TriggerKind::Command => ExtmarkKind::SlashCommand {
                                        cmd: item.insert_text.trim().to_string(),
                                    },
                                };
                                self.editor.replace_range_with_extmark(
                                    start,
                                    end,
                                    kind,
                                    &item.insert_text,
                                );
                                self.input.clear();
                                self.input.insert_str(&self.editor.text());
                                self.draft_snapshot = None;
                            }
                            self.popover.close();
                            self.dirty = true;
                            return None;
                        }
                        _ => {}
                    }
                }

                if key.code == KeyCode::Esc && self.editor.is_empty() && self.input.is_empty() {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }

                if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.cache.toggle_latest_thought();
                    self.dirty = true;
                    return None;
                }

                if key.code == KeyCode::Tab {
                    self.cache.toggle_latest_thought();
                    self.dirty = true;
                    return None;
                }

                if key.code == KeyCode::BackTab
                    || (key.code == KeyCode::Char('o')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    self.cache.toggle_latest_tool();
                    self.dirty = true;
                    return None;
                }

                let is_multiline_newline = (key.code == KeyCode::Enter
                    && (key.modifiers.contains(KeyModifiers::SHIFT)
                        || key.modifiers.contains(KeyModifiers::ALT)))
                    || (key.code == KeyCode::Char('j')
                        && key.modifiers.contains(KeyModifiers::CONTROL));

                if is_multiline_newline {
                    self.editor.insert_newline();
                    self.input.clear();
                    self.input.insert_str(&self.editor.text());
                    self.draft_snapshot = None;
                    self.dirty = true;
                    self.update_autocomplete();
                    return None;
                }

                match key.code {
                    KeyCode::Enter => {
                        let prompt = self
                            .editor
                            .take_submitted_expanded()
                            .or_else(|| self.input.take_submitted());
                        if let Some(p) = prompt {
                            self.input.clear();
                            self.draft_snapshot = None;
                            let trimmed = p.trim();
                            if trimmed == "/quit" || trimmed == "/exit" {
                                self.should_quit = true;
                                return Some(AppAction::Quit);
                            } else if trimmed == "/clear" {
                                self.cache.clear();
                                self.status_text = String::from("Cleared");
                                self.dirty = true;
                                return None;
                            } else if trimmed == "/interrupt" {
                                let _ = self.session_manager.interrupt_active();
                                self.status_text = String::from("Interrupted");
                                self.waiting_for_response = false;
                                self.dirty = true;
                                return Some(AppAction::Interrupt);
                            } else if trimmed == "/help" {
                                self.cache.push_user_message("/help");
                                self.cache.push_error("Available local commands: /clear, /interrupt, /quit, /exit, /help");
                                self.status_text = String::from("Ready");
                                self.dirty = true;
                                return None;
                            } else if trimmed.starts_with('/') {
                                self.cache.push_user_message(trimmed);
                                self.cache.push_error(&format!("Command '{}' is not supported locally. Type /help for available commands.", trimmed));
                                self.status_text = String::from("Ready");
                                self.dirty = true;
                                return None;
                            }

                            self.history.record_prompt(&p);
                            self.cache.push_user_message(&p);
                            self.viewport.scroll_to_bottom();
                            self.waiting_for_response = true;
                            self.dirty = true;
                            self.status_text = String::from("Waiting for response...");
                            return Some(AppAction::SubmitPrompt(p));
                        }
                    }
                    KeyCode::Backspace => {
                        self.editor.backspace();
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    KeyCode::Delete => {
                        self.editor.delete_forward();
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    KeyCode::Left => {
                        self.editor.move_left();
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    KeyCode::Right => {
                        self.editor.move_right();
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    KeyCode::Home => {
                        self.editor.move_home();
                        self.dirty = true;
                    }
                    KeyCode::End => {
                        self.editor.move_end();
                        self.dirty = true;
                    }
                    KeyCode::Up => {
                        if self.editor.move_up() {
                            self.dirty = true;
                        } else {
                            if self.draft_snapshot.is_none() {
                                self.draft_snapshot = Some(self.editor.snapshot());
                            }
                            let draft = self.editor.text();
                            if let Some(prev) = self.history.prev_history(&draft) {
                                self.editor.clear();
                                self.editor.insert_str(&prev);
                                self.input.clear();
                                self.input.insert_str(&prev);
                                self.dirty = true;
                            } else {
                                self.viewport.scroll_up(1);
                                self.dirty = true;
                            }
                        }
                    }
                    KeyCode::Down => {
                        if self.editor.move_down() {
                            self.dirty = true;
                        } else if let Some(next) = self.history.next_history() {
                            if self.history.is_navigating() {
                                self.editor.clear();
                                self.editor.insert_str(&next);
                                self.input.clear();
                                self.input.insert_str(&next);
                                self.dirty = true;
                            } else if let Some(draft) = self.draft_snapshot.take() {
                                self.editor.restore_snapshot(draft);
                                self.input.clear();
                                self.input.insert_str(&self.editor.text());
                                self.dirty = true;
                            } else {
                                self.editor.clear();
                                self.editor.insert_str(&next);
                                self.input.clear();
                                self.input.insert_str(&next);
                                self.dirty = true;
                            }
                        } else if let Some(draft) = self.draft_snapshot.take() {
                            self.editor.restore_snapshot(draft);
                            self.input.clear();
                            self.input.insert_str(&self.editor.text());
                            self.dirty = true;
                        } else {
                            self.viewport.scroll_down(1);
                            self.dirty = true;
                        }
                    }
                    KeyCode::PageUp => {
                        self.viewport.scroll_up(10);
                        self.dirty = true;
                    }
                    KeyCode::PageDown => {
                        self.viewport.scroll_down(10);
                        self.dirty = true;
                    }
                    KeyCode::Char(c) => {
                        self.editor.insert_char(c);
                        self.input.clear();
                        self.input.insert_str(&self.editor.text());
                        self.draft_snapshot = None;
                        self.dirty = true;
                        self.update_autocomplete();
                    }
                    _ => {}
                }
            }
            Event::Mouse(MouseEvent { kind, .. }) => match kind {
                MouseEventKind::ScrollUp => {
                    self.viewport.scroll_up(3);
                    self.dirty = true;
                }
                MouseEventKind::ScrollDown => {
                    self.viewport.scroll_down(3);
                    self.dirty = true;
                }
                _ => {}
            },
            Event::Paste(text) => {
                self.editor.insert_paste(&text);
                self.input.clear();
                self.input.insert_str(&self.editor.text());
                self.draft_snapshot = None;
                self.dirty = true;
                self.update_autocomplete();
            }
            Event::Resize(_, _) => {
                self.dirty = true;
            }
            _ => {}
        }
        None
    }

    pub fn update_autocomplete(&mut self) {
        if let Some((kind, start_char, query)) = self.editor.active_trigger() {
            match kind {
                TriggerKind::Command => {
                    let items = AutocompletePopover::filter_commands(&query);
                    self.popover.show(kind, start_char, &query, items);
                }
                TriggerKind::File => {
                    let items = self.search_worker.search(&query, 20);
                    self.popover.show(kind, start_char, &query, items);
                }
            }
        } else {
            self.popover.close();
        }
    }

    pub fn handle_engine_envelope(&mut self, envelope: SessionEnvelope) {
        if envelope.session_id != self.active_session {
            return;
        }

        match envelope.event {
            EngineEvent::Stream(boxed_ev) => match *boxed_ev {
                StreamEvent::Init(ev) => {
                    if let Some(conv_id) = ev.conversation_id {
                        self.status_text = format!("Conversation: {}", conv_id);
                    }
                    self.dirty = true;
                }
                StreamEvent::StepUpdate(update) => {
                    if let Some(data) = update.step_update {
                        if let Some(ref delta) = data.text_delta {
                            self.waiting_for_response = false;
                            self.cache
                                .append_streaming_delta(BlockKind::Assistant, delta);
                            self.status_text = String::from("Streaming...");
                            self.dirty = true;
                        }
                        if let Some(ref thought) = data.thought {
                            self.waiting_for_response = false;
                            self.cache
                                .append_streaming_delta(BlockKind::Thought, thought);
                            self.status_text = String::from("Thinking...");
                            self.dirty = true;
                        }
                        if let Some(tool_name) = data.resolved_tool_name() {
                            self.waiting_for_response = false;
                            let state = data.state.unwrap_or(StepState::Active);
                            let args = data
                                .tool_info
                                .as_ref()
                                .and_then(|info| info.parameters_text());
                            let output =
                                data.tool_info.as_ref().and_then(|info| info.output_text());
                            self.cache.upsert_tool_call(
                                data.step_index,
                                tool_name,
                                state,
                                args.as_deref(),
                                output.as_deref(),
                            );
                            self.status_text = format!("Tool: {}", tool_name);
                            self.dirty = true;
                        }
                    }
                }
                StreamEvent::Result(res) => {
                    self.waiting_for_response = false;
                    let fallback = res.result.as_ref().and_then(|r| r.response.as_deref());
                    self.cache.consolidate_active_turn(fallback);
                    self.status_text = String::from("Ready");
                    self.dirty = true;
                }
                StreamEvent::Error(err) => {
                    self.waiting_for_response = false;
                    let msg = err
                        .error
                        .or(err.message)
                        .unwrap_or_else(|| String::from("Unknown error"));
                    self.cache.push_error(&msg);
                    self.status_text = String::from("Error received");
                    self.dirty = true;
                }
                _ => {}
            },
            EngineEvent::Stderr(line) => {
                self.status_text = format!("stderr: {}", line);
                self.dirty = true;
            }
            EngineEvent::ProtocolError { message, .. } => {
                self.waiting_for_response = false;
                self.cache
                    .push_error(&format!("Protocol Error: {}", message));
                self.status_text = String::from("Protocol Error");
                self.dirty = true;
            }
            EngineEvent::ProcessExited { code, .. } => {
                self.waiting_for_response = false;
                self.status_text = format!("Process exited: {:?}", code);
                self.dirty = true;
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let input_lines = (self.editor.line_count() as u16).clamp(1, 8);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(input_lines + 2),
            ])
            .split(area);

        let viewport_area = chunks[0];
        let status_area = chunks[2];
        let input_area = chunks[3];

        let total_lines = self.cache.ensure_visual_lines(viewport_area.width);
        let total_with_spinner = total_lines + if self.waiting_for_response { 1 } else { 0 };
        self.viewport
            .sync_dimensions(total_with_spinner, viewport_area.height as usize);
        let mut visible_lines = self.viewport.visible_slice(&self.cache);

        if self.waiting_for_response {
            const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = SPINNER_FRAMES[(self.spinner_tick as usize / 4) % SPINNER_FRAMES.len()];
            visible_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", spinner),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "Thinking / Waiting for response...",
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        let viewport_widget = Paragraph::new(visible_lines);
        frame.render_widget(viewport_widget, viewport_area);

        let scroll_label = match self.viewport.mode() {
            ScrollMode::Anchored => "Auto-Scroll",
            ScrollMode::Unanchored { offset } => {
                let max = total_lines.saturating_sub(viewport_area.height as usize);
                if offset >= max {
                    "Bottom"
                } else {
                    "Scrolling"
                }
            }
        };

        let status_spans = vec![
            Span::styled(
                " FRONTAL ",
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                self.active_session.to_string(),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(" | "),
            Span::styled(&self.status_text, Style::default().fg(Color::Yellow)),
            Span::raw(" | "),
            Span::styled(scroll_label, Style::default().fg(Color::Cyan)),
        ];
        let status_widget = Paragraph::new(Line::from(status_spans));
        frame.render_widget(status_widget, status_area);

        let input_title = " Input (Enter: submit, Shift+Enter: newline, Ctrl+C: exit) ";
        let display_content = if !self.editor.is_empty() {
            self.editor.text()
        } else {
            self.input.content().to_string()
        };

        let (cur_line, cur_col) = if !self.editor.is_empty() {
            self.editor.cursor_coordinates()
        } else {
            (0, self.input.cursor_char_idx())
        };

        let scroll_y = if cur_line >= 8 {
            (cur_line - 7) as u16
        } else {
            0
        };

        let mut input_widget = Paragraph::new(display_content).block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        if scroll_y > 0 {
            input_widget = input_widget.scroll((scroll_y, 0));
        }
        frame.render_widget(input_widget, input_area);

        let cursor_x = input_area
            .x
            .saturating_add(1)
            .saturating_add(cur_col as u16);
        let cursor_y = input_area
            .y
            .saturating_add(1)
            .saturating_add(cur_line.saturating_sub(scroll_y as usize) as u16);
        if cursor_x
            < input_area
                .x
                .saturating_add(input_area.width.saturating_sub(1))
            && cursor_y
                < input_area
                    .y
                    .saturating_add(input_area.height.saturating_sub(1))
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        if self.popover.is_visible() {
            let item_count = self.popover.items.len().min(8) as u16;
            let popover_height = item_count + 2;
            let popover_width = (input_area.width.saturating_sub(4)).min(60);
            let popover_y = input_area.y.saturating_sub(popover_height);
            let popover_rect = Rect {
                x: input_area.x.saturating_add(2),
                y: popover_y,
                width: popover_width,
                height: popover_height,
            }
            .intersection(area);

            frame.render_widget(Clear, popover_rect);

            let mut popover_lines = Vec::new();
            for (idx, item) in self
                .popover
                .items
                .iter()
                .enumerate()
                .take(item_count as usize)
            {
                let is_sel = idx == self.popover.selected_idx;
                let (prefix, style) = if is_sel {
                    (
                        " > ",
                        Style::default()
                            .bg(Color::Cyan)
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("   ", Style::default().fg(Color::White))
                };
                popover_lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(&item.label, style),
                ]));
            }

            let title = match self.popover.kind {
                TriggerKind::Command => " Commands (Tab/Enter: apply, Esc: close) ",
                TriggerKind::File => " Files (Tab/Enter: apply, Esc: close) ",
            };

            let popover_widget = Paragraph::new(popover_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Yellow)),
            );
            frame.render_widget(popover_widget, popover_rect);
        }
    }
}
