use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::protocol::{EngineEvent, SessionEnvelope, SessionId, StepState, StreamEvent};
use crate::session::{SessionManager, SessionStatus};
use crate::ui::cache::{BlockKind, RenderCache};
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
    pub dirty: bool,
    pub should_quit: bool,
    pub status_text: String,
}

impl App {
    pub fn new(session_manager: SessionManager, active_session: SessionId) -> Self {
        Self {
            session_manager,
            active_session,
            cache: RenderCache::new(),
            viewport: ConversationViewport::new(),
            input: MinimalInputStub::new(),
            dirty: true,
            should_quit: false,
            status_text: String::from("Ready"),
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

                if key.code == KeyCode::Esc && self.input.content().is_empty() {
                    self.should_quit = true;
                    return Some(AppAction::Quit);
                }

                match key.code {
                    KeyCode::Enter => {
                        if let Some(prompt) = self.input.take_submitted() {
                            self.cache.push_user_message(&prompt);
                            self.viewport.scroll_to_bottom();
                            self.dirty = true;
                            self.status_text = String::from("Running...");
                            return Some(AppAction::SubmitPrompt(prompt));
                        }
                    }
                    KeyCode::Backspace => {
                        self.input.backspace();
                        self.dirty = true;
                    }
                    KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.cache.toggle_latest_thought();
                        self.dirty = true;
                    }
                    KeyCode::Tab => {
                        self.cache.toggle_latest_thought();
                        self.dirty = true;
                    }
                    KeyCode::Char(c) => {
                        self.input.insert_char(c);
                        self.dirty = true;
                    }
                    KeyCode::Up => {
                        self.viewport.scroll_up(1);
                        self.dirty = true;
                    }
                    KeyCode::Down => {
                        self.viewport.scroll_down(1);
                        self.dirty = true;
                    }
                    KeyCode::PageUp => {
                        self.viewport.scroll_up(10);
                        self.dirty = true;
                    }
                    KeyCode::PageDown => {
                        self.viewport.scroll_down(10);
                        self.dirty = true;
                    }
                    KeyCode::Home => {
                        self.viewport.scroll_up(usize::MAX / 2);
                        self.dirty = true;
                    }
                    KeyCode::End => {
                        self.viewport.scroll_to_bottom();
                        self.dirty = true;
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
                self.input.insert_str(&text);
                self.dirty = true;
            }
            Event::Resize(_, _) => {
                self.dirty = true;
            }
            _ => {}
        }
        None
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
                            self.cache
                                .append_streaming_delta(BlockKind::Assistant, delta);
                            self.status_text = String::from("Streaming...");
                            self.dirty = true;
                        }
                        if let Some(ref thought) = data.thought {
                            self.cache
                                .append_streaming_delta(BlockKind::Thought, thought);
                            self.status_text = String::from("Thinking...");
                            self.dirty = true;
                        }
                        if let Some(tool_name) = data.resolved_tool_name() {
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
                    let fallback = res.result.as_ref().and_then(|r| r.response.as_deref());
                    self.cache.consolidate_active_turn(fallback);
                    self.status_text = String::from("Ready");
                    self.dirty = true;
                }
                StreamEvent::Error(err) => {
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
                self.cache
                    .push_error(&format!("Protocol Error: {}", message));
                self.status_text = String::from("Protocol Error");
                self.dirty = true;
            }
            EngineEvent::ProcessExited { code, .. } => {
                self.status_text = format!("Process exited: {:?}", code);
                self.dirty = true;
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .split(area);

        let viewport_area = chunks[0];
        let status_area = chunks[1];
        let input_area = chunks[2];

        let total_lines = self.cache.ensure_visual_lines(viewport_area.width);
        self.viewport
            .sync_dimensions(total_lines, viewport_area.height as usize);
        let visible_lines = self.viewport.visible_slice(&self.cache);
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

        let input_title = " Input (Enter: submit, Ctrl+C: interrupt/exit) ";
        let input_widget = Paragraph::new(self.input.content()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(input_title)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(input_widget, input_area);

        let cursor_x = input_area
            .x
            .saturating_add(1)
            .saturating_add(self.input.cursor_char_idx() as u16);
        let cursor_y = input_area.y.saturating_add(1);
        if cursor_x
            < input_area
                .x
                .saturating_add(input_area.width.saturating_sub(1))
        {
            frame.set_cursor_position((cursor_x, cursor_y));
        }

        self.dirty = false;
    }
}
