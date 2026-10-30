pub mod app;
pub mod autocomplete;
pub mod cache;
pub mod editor;
pub mod highlight;
pub mod history;
pub mod input_stub;
pub mod markdown;
pub mod terminal;
pub mod viewport;

use crossterm::event::EventStream;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::time::{interval, MissedTickBehavior};

use crate::ui::app::{App, AppAction};
use crate::ui::terminal::TerminalGuard;

pub async fn run_tui(mut app: App) -> Result<(), Box<dyn std::error::Error>> {
    let mut guard = TerminalGuard::enter()?;
    let mut event_stream = EventStream::new();
    let mut ticker = interval(Duration::from_micros(16_666));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    guard.terminal_mut().draw(|f| app.render(f))?;

    while !app.should_quit {
        tokio::select! {
            maybe_event = event_stream.next() => {
                if let Some(Ok(event)) = maybe_event {
                    if let Some(action) = app.handle_terminal_event(event) {
                        match action {
                            AppAction::SubmitPrompt(prompt) => {
                                let _ = app.session_manager.send_prompt(&app.active_session, &prompt).await;
                            }
                            AppAction::Interrupt => {
                                let _ = app.session_manager.interrupt_active();
                            }
                            AppAction::Quit => {
                                break;
                            }
                        }
                    }
                }
            }
            maybe_envelope = app.session_manager.recv() => {
                if let Some(envelope) = maybe_envelope {
                    app.handle_engine_envelope(envelope);
                }
            }
            _ = ticker.tick() => {
                app.spinner_tick = app.spinner_tick.wrapping_add(1);
                if app.waiting_for_response {
                    app.dirty = true;
                }
                if app.dirty {
                    guard.terminal_mut().draw(|f| app.render(f))?;
                    app.dirty = false;
                }
            }
        }
    }

    guard.restore();
    let _ = app
        .session_manager
        .shutdown_all(Duration::from_millis(500))
        .await;
    Ok(())
}
