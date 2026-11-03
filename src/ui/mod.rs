pub mod app;
pub mod autocomplete;
pub mod cache;
pub mod clipboard;
pub mod diff;
pub mod editor;
pub mod highlight;
pub mod history;
pub mod markdown;
pub mod modal;
pub mod notify;
pub mod tab;
pub mod terminal;
pub mod theme;
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

    let (models_tx, mut models_rx) = tokio::sync::mpsc::channel(1);
    let agy_bin = crate::transport::EngineConfig::resolve_agy_binary();
    tokio::spawn(async move {
        let models = crate::transport::EngineConfig::fetch_available_models(&agy_bin).await;
        let _ = models_tx.send(models).await;
    });

    guard.terminal_mut().draw(|f| app.render(f))?;

    while !app.should_quit {
        tokio::select! {
            Some(models) = models_rx.recv() => {
                app.set_available_models(models);
            }
            maybe_event = event_stream.next() => {
                if let Some(Ok(event)) = maybe_event {
                    if let Some(action) = app.handle_terminal_event(event) {
                        match action {
                            AppAction::SubmitPrompt(prompt) => {
                                let _ = app
                                    .session_manager
                                    .send_prompt(&app.active_session, &prompt)
                                    .await;
                            }
                            AppAction::Interrupt => {
                                let _ = app.session_manager.interrupt_active();
                            }
                            AppAction::NewTab => {
                                let mut config = app
                                    .session_manager
                                    .get_engine_config(&app.active_session)
                                    .unwrap_or_default();
                                config.conversation_id = None;
                                config.model = Some(app.active_model_name());
                                config.effort = Some(app.active_effort_name());
                                config.mode = Some(app.active_mode_name());
                                if let Ok(new_id) =
                                    app.session_manager.create_subprocess_session(config).await
                                {
                                    app.add_tab(new_id, None);
                                }
                            }
                            AppAction::SwitchTab(idx) => {
                                if app.active_tab_index != idx {
                                    app.switch_tab(idx);
                                }
                            }
                            AppAction::CloseTab(idx) => {
                                let _ = app.close_tab(idx).await;
                            }
                            AppAction::RespawnSession {
                                conversation_id,
                                model,
                                effort,
                                mode,
                            } => {
                                let _ = app
                                    .respawn_active_session(conversation_id, model, effort, mode)
                                    .await;
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
                if app.waiting_for_response || app.tabs.iter().any(|t| t.waiting_for_response) {
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
