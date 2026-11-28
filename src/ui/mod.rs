pub mod app;
pub mod autocomplete;
pub mod cache;
pub mod editor;
pub mod highlight;
pub mod history;
pub mod labels;
pub mod markdown;
pub mod modal;
pub mod notify;
pub mod tab;
pub mod terminal;
pub mod theme;
pub mod viewport;

pub use crate::infra::clipboard;

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
    let engine_config = app
        .session_manager
        .get_engine_config(app.active_session_id());
    let agy_bin = engine_config
        .as_ref()
        .map(|config| config.agy_bin.clone())
        .unwrap_or_else(crate::transport::EngineConfig::resolve_agy_binary);
    tokio::spawn(async move {
        let models = crate::transport::EngineConfig::fetch_available_models(&agy_bin).await;
        let _ = models_tx.send(models).await;
    });

    let (catalog_tx, mut catalog_rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        let catalog = tokio::task::spawn_blocking(crate::infra::model_catalog::load_default)
            .await
            .unwrap_or_default();
        let _ = catalog_tx.send(catalog).await;
    });

    let (opencode_tx, mut opencode_rx) = tokio::sync::mpsc::channel(1);
    let opencode_bin = engine_config
        .as_ref()
        .map(|config| config.opencode_bin.clone())
        .unwrap_or_else(crate::transport::resolve_opencode_binary);
    tokio::spawn(async move {
        let models = crate::transport::fetch_opencode_models(&opencode_bin).await;
        let _ = opencode_tx.send(models).await;
    });

    let (paste_tx, mut paste_rx) = tokio::sync::mpsc::channel(8);
    let (bg_tx, mut bg_rx) = tokio::sync::mpsc::channel::<BgEvent>(16);
    let mut bg_tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    let (osc52_tx, mut osc52_rx) = tokio::sync::watch::channel::<(u64, String)>((0, String::new()));
    let mut applied_osc52_seq: u64 = 0;
    let mut pending_osc52: Option<(u64, String)> = None;

    let mut run_result: Result<(), Box<dyn std::error::Error>> = guard
        .terminal_mut()
        .draw(|f| app.render(f))
        .map(|_| ())
        .map_err(Into::into);

    while run_result.is_ok() && !app.should_quit {
        tokio::select! {
            Some(models) = models_rx.recv() => {
                app.set_available_models(models);
            }
            Some(models) = opencode_rx.recv() => {
                app.set_opencode_models(models);
            }
            Some(catalog) = catalog_rx.recv() => {
                app.set_model_catalog(catalog);
            }
            Some((token, paste_res)) = paste_rx.recv() => {
                app.apply_paste_outcome(token, paste_res);
            }
            Some(bg_event) = bg_rx.recv() => {
                while bg_tasks.try_join_next().is_some() {}
                handle_bg_event(&mut app, bg_event);
            }
            Ok(()) = osc52_rx.changed() => {
                sync_osc52_watch(&mut osc52_rx, &mut applied_osc52_seq, &mut pending_osc52, &app.clipboard);
            }
            maybe_event = event_stream.next() => {
                if let Some(Ok(event)) = maybe_event {
                    if let Some(action) = app.handle_terminal_event(event) {
                        match action {
                            AppAction::SubmitPrompt(prompt) => {
                                let session_id = app.active_session_id().clone();
                                if app.session_manager.is_dormant(&session_id) {
                                    let (plan, io_input) =
                                        app.prepare_respawn(None, None, None, None);
                                    let tab = app.active_tab_mut();
                                    tab.transition_turn(crate::ui::tab::TurnEvent::Submit);
                                    tab.status_text = String::from("Resuming session...");
                                    app.dirty = true;
                                    let tx = bg_tx.clone();
                                    bg_tasks.spawn(async move {
                                        let outcome = Box::new(app::run_respawn_io(io_input).await);
                                        send_bg_or_discard(
                                            &tx,
                                            BgEvent::RespawnFinished {
                                                plan: Box::new(plan),
                                                outcome,
                                                follow_up_prompt: Some(prompt),
                                            },
                                        )
                                        .await;
                                    });
                                } else if let Err(err) =
                                    app.session_manager.dispatch_prompt_sync(&session_id, prompt)
                                {
                                    app.handle_prompt_failure(&session_id, &err.to_string());
                                }
                            }
                            AppAction::PasteClipboard => {
                                let token = app.active_op_token();
                                let cache_dir = app.clipboard.resolve_media_cache_dir();
                                let tx = paste_tx.clone();
                                tokio::task::spawn_blocking(move || {
                                    let _ = tx.blocking_send((
                                        token,
                                        resolve_clipboard_paste(cache_dir),
                                    ));
                                });
                            }
                            AppAction::InspectPastedText(text) => {
                                let token = app.active_op_token();
                                let cache_dir = app.clipboard.resolve_media_cache_dir();
                                let tx = paste_tx.clone();
                                tokio::task::spawn_blocking(move || {
                                    let res = if text.is_empty() {
                                        resolve_clipboard_paste(cache_dir)
                                    } else {
                                        Ok(App::resolve_pasted_text(text))
                                    };
                                    let _ = tx.blocking_send((token, res));
                                });
                            }
                            AppAction::CopyClipboard(text) => {
                                if app.clipboard.is_isolated() {
                                    let _ = app.clipboard.set_text(&text);
                                } else {
                                    let (seq, write_fn) = app.clipboard.prepare_background_write();
                                    let tx = osc52_tx.clone();
                                    tokio::task::spawn_blocking(move || {
                                        if let Some(fallback_text) = write_fn(text) {
                                            tx.send_if_modified(|current| {
                                                if seq > current.0 {
                                                    *current = (seq, fallback_text);
                                                    true
                                                } else {
                                                    false
                                                }
                                            });
                                        }
                                    });
                                }
                            }
                            AppAction::Interrupt => {
                                app.apply_sync_action(&AppAction::Interrupt);
                            }
                            AppAction::NewTab => {
                                let mut config = app
                                    .session_manager
                                    .get_engine_config(app.active_session_id())
                                    .unwrap_or_else(|| {
                                        crate::transport::EngineConfig::from_user_config(
                                            &app.user_config,
                                        )
                                    });
                                config.conversation_id = None;
                                config.model = Some(app.active_model_name());
                                config.effort = Some(app.active_effort_name());
                                config.mode = Some(app.active_mode_name());
                                let sender = app.session_manager.event_sender();
                                let tx = bg_tx.clone();
                                bg_tasks.spawn(async move {
                                    let res = crate::infra::session::SessionManager::spawn_engine_detached(
                                        config,
                                        sender,
                                    )
                                    .await;
                                    send_bg_or_discard(&tx, BgEvent::NewTabSpawned { res }).await;
                                });
                            }
                            AppAction::SwitchTab(idx) => {
                                app.apply_sync_action(&AppAction::SwitchTab(idx));
                            }
                            AppAction::CloseTab(idx) => {
                                let _ = app.close_tab_sync(idx);
                            }
                            AppAction::CloseOrResetTab(idx) => {
                                if let Some(pending) = app.close_or_reset_tab_sync(idx) {
                                    let tx = bg_tx.clone();
                                    bg_tasks.spawn(async move {
                                        let res = crate::infra::session::SessionManager::spawn_engine_detached(
                                            pending.config.clone(),
                                            pending.event_sender.clone(),
                                        )
                                        .await;
                                        send_bg_or_discard(
                                            &tx,
                                            BgEvent::TabResetSpawned {
                                                pending: Box::new(pending),
                                                res,
                                            },
                                        )
                                        .await;
                                    });
                                }
                            }
                            AppAction::RespawnSession {
                                conversation_id,
                                model,
                                effort,
                                mode,
                            } => {
                                let (plan, io_input) =
                                    app.prepare_respawn(conversation_id, model, effort, mode);
                                app.active_tab_mut().status_text =
                                    String::from("Respawning session...");
                                app.dirty = true;
                                let tx = bg_tx.clone();
                                bg_tasks.spawn(async move {
                                    let outcome = Box::new(app::run_respawn_io(io_input).await);
                                    send_bg_or_discard(
                                        &tx,
                                        BgEvent::RespawnFinished {
                                            plan: Box::new(plan),
                                            outcome,
                                            follow_up_prompt: None,
                                        },
                                    )
                                    .await;
                                });
                            }
                            AppAction::Quit => {
                                break;
                            }
                        }
                    }
                    app.flush_pending_engine_prompts();
                }
            }
            maybe_envelope = app.session_manager.recv() => {
                if let Some(envelope) = maybe_envelope {
                    app.handle_engine_envelope(envelope);
                    app.flush_pending_engine_prompts();
                }
            }
            _ = ticker.tick() => {
                app.spinner_tick = app.spinner_tick.wrapping_add(1);
                let timers_expired = app.expire_interrupt_timers();
                let titles_updated = app.apply_pending_agent_updates();
                if timers_expired || titles_updated {
                    app.dirty = true;
                }
                let spinner_active = app.active_tab().is_waiting_for_response()
                    || app.tabs.iter().any(|t| {
                        t.is_waiting_for_response() || t.cache.has_unconsolidated_blocks()
                    });
                if (spinner_active && app.spinner_tick.is_multiple_of(4))
                    || app.modals.is_top_loading()
                {
                    app.dirty = true;
                }
                if app.dirty {
                    if let Err(err) = guard.terminal_mut().draw(|f| app.render(f)) {
                        run_result = Err(err.into());
                        break;
                    }
                    app.dirty = false;
                }
                let mut writes = app.take_pending_terminal_writes();
                if let Some(osc) = take_pending_osc52_bytes(&mut pending_osc52, &app.clipboard) {
                    writes.push(osc);
                }
                if let Err(err) = guard.write_raw_sequences(&writes) {
                    run_result = Err(err.into());
                    break;
                }
            }
        }
    }

    if osc52_rx.has_changed().unwrap_or(false) {
        sync_osc52_watch(
            &mut osc52_rx,
            &mut applied_osc52_seq,
            &mut pending_osc52,
            &app.clipboard,
        );
    }
    let mut final_writes = app.take_pending_terminal_writes();
    if let Some(osc) = take_pending_osc52_bytes(&mut pending_osc52, &app.clipboard) {
        final_writes.push(osc);
    }
    let _ = guard.write_raw_sequences(&final_writes);
    guard.restore();
    bg_rx.close();
    drop(bg_tx);
    while let Ok(bg_event) = bg_rx.try_recv() {
        bg_tasks.spawn(discard_bg_event(bg_event));
    }
    let history_flusher = app.history.hydration_client();
    let flush_task = tokio::task::spawn_blocking(move || {
        let _ = history_flusher.flush_with_timeout(Duration::from_millis(500));
    });
    let (_, _, _) = tokio::join!(
        app.session_manager.shutdown_all(Duration::from_millis(500)),
        drain_bg_tasks(&mut bg_tasks, Duration::from_millis(800)),
        flush_task,
    );
    let _ = crate::engine::drain_pgid_reaper(Duration::from_millis(300));
    run_result
}

async fn drain_bg_tasks(bg_tasks: &mut tokio::task::JoinSet<()>, wait: Duration) {
    let drained = tokio::time::timeout(wait, async {
        while bg_tasks.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        bg_tasks.abort_all();
        while bg_tasks.join_next().await.is_some() {}
    }
}

enum BgEvent {
    NewTabSpawned {
        res: Result<
            (
                crate::protocol::SessionId,
                crate::infra::session::SessionHandle,
            ),
            crate::transport::EngineError,
        >,
    },
    TabResetSpawned {
        pending: Box<app::PendingTabReset>,
        res: Result<
            (
                crate::protocol::SessionId,
                crate::infra::session::SessionHandle,
            ),
            crate::transport::EngineError,
        >,
    },
    RespawnFinished {
        plan: Box<app::RespawnPlan>,
        outcome: Box<app::RespawnIoResult>,
        follow_up_prompt: Option<crate::protocol::PromptPayload>,
    },
}

fn handle_bg_event(app: &mut App, event: BgEvent) {
    match event {
        BgEvent::NewTabSpawned { res } => match res {
            Ok((new_id, handle)) => {
                app.session_manager
                    .attach_engine(new_id.clone(), handle, false);
                app.add_tab(new_id, None);
            }
            Err(err) => {
                app.active_tab_mut().status_text = format!("Failed to open tab: {}", err);
                app.dirty = true;
            }
        },
        BgEvent::TabResetSpawned { pending, res } => {
            app.finish_tab_reset(*pending, res);
        }
        BgEvent::RespawnFinished {
            plan,
            outcome,
            follow_up_prompt,
        } => {
            let token = plan.token;
            match app.apply_respawn_outcome(*plan, *outcome) {
                Ok(Some(new_id)) => {
                    if let Some(prompt) = follow_up_prompt {
                        if let Some(tab) = app.tabs.iter_mut().find(|t| {
                            t.tab_id == token.tab_id && t.generation_id == token.generation_id
                        }) {
                            tab.transition_turn(crate::ui::tab::TurnEvent::Submit);
                            tab.status_text = String::from("Thinking...");
                        }
                        app.dirty = true;
                        if let Err(err) = app.session_manager.dispatch_prompt_sync(&new_id, prompt)
                        {
                            app.handle_prompt_failure(&new_id, &err.to_string());
                        }
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    if follow_up_prompt.is_some() {
                        if let Some(tab_idx) = app.tabs.iter().position(|t| {
                            t.tab_id == token.tab_id && t.generation_id == token.generation_id
                        }) {
                            let sid = app.tabs[tab_idx].session_id.clone();
                            app.handle_prompt_failure(&sid, &err.to_string());
                        }
                    }
                }
            }
        }
    }
}

fn sync_osc52_watch(
    osc52_rx: &mut tokio::sync::watch::Receiver<(u64, String)>,
    applied_seq: &mut u64,
    pending: &mut Option<(u64, String)>,
    clipboard: &clipboard::SystemClipboard,
) {
    let guard = osc52_rx.borrow_and_update();
    if guard.0 > *applied_seq && guard.0 >= clipboard.last_applied_seq() && !guard.1.is_empty() {
        *applied_seq = guard.0;
        *pending = Some((guard.0, guard.1.clone()));
    }
}

fn take_pending_osc52_bytes(
    pending: &mut Option<(u64, String)>,
    clipboard: &clipboard::SystemClipboard,
) -> Option<Vec<u8>> {
    let (seq, text) = pending.take()?;
    (seq >= clipboard.last_applied_seq())
        .then(|| clipboard::SystemClipboard::format_osc52(&text).into_bytes())
}

async fn send_bg_or_discard(tx: &tokio::sync::mpsc::Sender<BgEvent>, event: BgEvent) {
    if let Err(err) = tx.send(event).await {
        discard_bg_event(err.0).await;
    }
}

async fn discard_bg_event(event: BgEvent) {
    let maybe_handle = match event {
        BgEvent::NewTabSpawned { res: Ok((_, h)) } => Some(h),
        BgEvent::TabResetSpawned {
            res: Ok((_, h)), ..
        } => Some(h),
        BgEvent::RespawnFinished { outcome, .. } => match outcome.spawned {
            Some(Ok((_, h))) => Some(h),
            _ => None,
        },
        _ => None,
    };
    if let Some(handle) = maybe_handle {
        let _ = crate::infra::session::SessionManager::shutdown_handle(
            handle,
            Duration::from_millis(500),
        )
        .await;
    }
}

fn resolve_clipboard_paste(
    cache_dir: Option<std::path::PathBuf>,
) -> Result<
    (
        Option<crate::ui::clipboard::SavedMediaImage>,
        Option<String>,
    ),
    String,
> {
    crate::ui::clipboard::read_system_clipboard_media_or_text(cache_dir).map(|(img, txt)| {
        match (img, txt) {
            (Some(saved), _) => (Some(saved), None),
            (None, Some(raw)) => App::resolve_pasted_text(raw),
            (None, None) => (None, None),
        }
    })
}
