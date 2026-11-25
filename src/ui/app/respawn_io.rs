use crate::config::BackendKind;
use crate::infra::history::HydrationClient;
use crate::infra::session::{SessionHandle, SessionManager};
use crate::protocol::{SessionEnvelope, SessionId};
use crate::transport::{EngineConfig, EngineError};
use crate::ui::app::respawn::is_model_compatible_with_backend;
use crate::ui::history::{hydrate_session_with_client, HydratedSession};

pub(crate) struct RespawnIoInput {
    pub detached_old: Option<SessionHandle>,
    pub should_hydrate: bool,
    pub final_conv_id: Option<String>,
    pub hydration_client: HydrationClient,
    pub engine_config: Option<EngineConfig>,
    pub event_sender: tokio::sync::mpsc::Sender<SessionEnvelope>,
    pub target_backend: BackendKind,
    pub requested_model: Option<String>,
    pub requested_effort: Option<String>,
    pub inherited_prev_model: Option<String>,
    pub inherited_prev_effort: Option<String>,
    pub user_fallback_model: Option<String>,
    pub user_config_effort: String,
}

pub(crate) struct RespawnIoResult {
    pub hydrated_session: Option<HydratedSession>,
    pub final_model: Option<String>,
    pub final_effort: Option<String>,
    pub final_mode: Option<String>,
    pub spawned: Option<Result<(SessionId, SessionHandle), EngineError>>,
    pub warnings: Vec<String>,
}

fn resolve_hydrated_model_and_effort(
    hydrated_session: Option<&HydratedSession>,
    target_backend: BackendKind,
) -> (Option<String>, Option<String>) {
    let hydrated_effort_direct = hydrated_session
        .and_then(|h| h.effort.as_deref())
        .and_then(|e| target_backend.sanitize_effort(e));
    hydrated_session
        .and_then(|h| h.telemetry.as_ref())
        .and_then(|t| t.model.as_deref())
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(|raw| {
            let sanitized = crate::config::sanitize_model_id(raw);
            if target_backend == BackendKind::Agy && !sanitized.contains('/') {
                let (base, eff) = EngineConfig::split_model_effort(&sanitized);
                let model = (!base.is_empty()).then(|| base.to_string());
                let effort = model.as_ref().and_then(|_| {
                    eff.and_then(|e| target_backend.sanitize_effort(e))
                        .or_else(|| hydrated_effort_direct.clone())
                });
                (model, effort)
            } else if !sanitized.is_empty()
                && is_model_compatible_with_backend(target_backend, &sanitized)
            {
                (Some(sanitized), hydrated_effort_direct.clone())
            } else {
                (None, None)
            }
        })
        .unwrap_or((None, None))
}

fn resolve_final_model_and_effort(
    target_backend: BackendKind,
    hydrated: (Option<String>, Option<String>),
    requested: (Option<String>, Option<String>),
    inherited: (Option<String>, Option<String>),
    user_fallback: (Option<String>, String),
) -> (Option<String>, Option<String>) {
    let clean_req = requested
        .1
        .as_deref()
        .and_then(|e| target_backend.sanitize_effort(e));
    if requested.0.is_some() {
        return (requested.0, clean_req);
    }
    if hydrated.0.is_some() {
        let clean_hyd = hydrated
            .1
            .as_deref()
            .and_then(|e| target_backend.sanitize_effort(e));
        return (hydrated.0, clean_req.or(clean_hyd));
    }
    if inherited.0.is_some() {
        let clean_inh = inherited
            .1
            .as_deref()
            .and_then(|e| target_backend.sanitize_effort(e));
        return (inherited.0, clean_req.or(clean_inh));
    }
    if user_fallback.0.is_some() {
        let clean_cfg = target_backend.sanitize_effort(&user_fallback.1);
        return (user_fallback.0, clean_req.or(clean_cfg));
    }
    (None, clean_req)
}

pub(crate) async fn run_respawn_io(input: RespawnIoInput) -> RespawnIoResult {
    let mut warnings = Vec::new();
    if let Some(old_handle) = input.detached_old {
        if let Err(err) =
            SessionManager::shutdown_handle(old_handle, std::time::Duration::from_millis(500)).await
        {
            warnings.push(format!("Previous session shutdown failed: {}", err));
        }
    }

    let hydrated_session = if input.should_hydrate {
        if let Some(cid_clone) = input.final_conv_id {
            let client = input.hydration_client;
            match tokio::task::spawn_blocking(move || {
                hydrate_session_with_client(&cid_clone, &client)
            })
            .await
            {
                Ok(hydrated) => Some(hydrated),
                Err(err) => {
                    warnings.push(format!("Hydration task failed: {}", err));
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let hydrated =
        resolve_hydrated_model_and_effort(hydrated_session.as_ref(), input.target_backend);
    let (final_model, final_effort) = resolve_final_model_and_effort(
        input.target_backend,
        hydrated,
        (input.requested_model, input.requested_effort),
        (input.inherited_prev_model, input.inherited_prev_effort),
        (input.user_fallback_model, input.user_config_effort),
    );
    let final_mode = input.engine_config.as_ref().and_then(|c| c.mode.clone());

    let spawned = if let Some(mut cfg) = input.engine_config {
        cfg.model = final_model.clone();
        cfg.effort = final_effort.clone();
        Some(SessionManager::spawn_engine_detached(cfg, input.event_sender).await)
    } else {
        None
    };

    RespawnIoResult {
        hydrated_session,
        final_model,
        final_effort,
        final_mode,
        spawned,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::history::{TelemetrySnapshot, TitleSource};
    use crate::protocol::TokenUsage;
    use crate::ui::cache::RenderCache;

    fn hydrated_with(model: Option<&str>, effort: Option<&str>) -> HydratedSession {
        HydratedSession {
            hydrated: false,
            cache: RenderCache::new(),
            title: None,
            is_custom_title: false,
            title_source: TitleSource::Default,
            effort: effort.map(str::to_string),
            telemetry: model.map(|m| TelemetrySnapshot {
                last: TokenUsage::default(),
                accumulated: TokenUsage::default(),
                steps: 0,
                turns: 0,
                duration_millis: None,
                context_window: None,
                model: Some(m.to_string()),
            }),
        }
    }

    #[test]
    fn hydrated_effort_sanitizes_dirty_values_and_drops_when_model_absent() {
        let missing_model = hydrated_with(None, Some("xhigh"));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&missing_model), BackendKind::Agy),
            (None, None)
        );
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&missing_model), BackendKind::OpenCode),
            (None, None)
        );

        let dirty_direct = hydrated_with(Some("anthropic/claude-sonnet-4"), Some("bogus!"));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&dirty_direct), BackendKind::OpenCode),
            (Some("anthropic/claude-sonnet-4".to_string()), None)
        );

        let valid_direct = hydrated_with(Some("anthropic/claude-sonnet-4"), Some(" HIGH "));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&valid_direct), BackendKind::OpenCode),
            (
                Some("anthropic/claude-sonnet-4".to_string()),
                Some("high".to_string()),
            )
        );

        let agy_suffix = hydrated_with(Some("gemini-3.8-flash-xhigh"), Some("bogus!"));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&agy_suffix), BackendKind::Agy),
            (
                Some("gemini-3.8-flash".to_string()),
                Some("xhigh".to_string()),
            )
        );

        let agy_dirty_direct = hydrated_with(Some("gemini-3.8-flash"), Some("not_an_effort"));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&agy_dirty_direct), BackendKind::Agy),
            (Some("gemini-3.8-flash".to_string()), None)
        );

        let incompatible_model = hydrated_with(Some("openai/gpt-5"), Some("high"));
        assert_eq!(
            resolve_hydrated_model_and_effort(Some(&incompatible_model), BackendKind::Agy),
            (None, None)
        );
    }

    #[test]
    fn final_effort_inherits_prev_or_user_config_when_hydrated_model_absent() {
        let missing_model = hydrated_with(None, Some("low"));
        let hm = resolve_hydrated_model_and_effort(Some(&missing_model), BackendKind::Agy);
        let (model, effort) = resolve_final_model_and_effort(
            BackendKind::Agy,
            hm,
            (None, None),
            (
                Some("gemini-3.8-flash".to_string()),
                Some("medium".to_string()),
            ),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort.as_deref(), Some("medium"));

        let hm2 = resolve_hydrated_model_and_effort(Some(&missing_model), BackendKind::Agy);
        let (model2, effort2) = resolve_final_model_and_effort(
            BackendKind::Agy,
            hm2,
            (None, None),
            (None, None),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model2.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort2.as_deref(), Some("max"));

        let model_no_effort = hydrated_with(Some("gemini-3.8-flash"), None);
        let hm3 = resolve_hydrated_model_and_effort(Some(&model_no_effort), BackendKind::Agy);
        let (model3, effort3) = resolve_final_model_and_effort(
            BackendKind::Agy,
            hm3,
            (None, None),
            (
                Some("gemini-3.8-flash".to_string()),
                Some("medium".to_string()),
            ),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model3.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort3, None);

        let (model4, effort4) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (Some("gemini-3.8-flash".to_string()), None),
            (None, Some(" XHIGH! ".to_string())),
            (None, Some("low".to_string())),
            (Some("gemini-3.8-flash".to_string()), "bad/cfg".to_string()),
        );
        assert_eq!(model4.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort4.as_deref(), Some("xhigh"));

        let (_model5, effort5) = resolve_final_model_and_effort(
            BackendKind::OpenCode,
            (None, None),
            (None, Some("bogus".to_string())),
            (None, Some("h/igh".to_string())),
            (
                Some("anthropic/claude-sonnet-4".to_string()),
                "bad!".to_string(),
            ),
        );
        assert_eq!(effort5, None);

        let (model6, effort6) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (
                Some("gemini-3.8-flash".to_string()),
                Some("xhigh".to_string()),
            ),
            (Some("gemini-3.8-pro".to_string()), None),
            (None, Some("low".to_string())),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model6.as_deref(), Some("gemini-3.8-pro"));
        assert_eq!(effort6, None);

        let (model7, effort7) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (None, None),
            (None, None),
            (None, Some("low".to_string())),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model7.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort7.as_deref(), Some("max"));

        let (model8, effort8) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (
                Some("gemini-3.8-flash".to_string()),
                Some("h/igh".to_string()),
            ),
            (None, None),
            (None, None),
            (None, "max".to_string()),
        );
        assert_eq!(model8.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort8, None);

        let (model9, effort9) = resolve_final_model_and_effort(
            BackendKind::OpenCode,
            (None, None),
            (None, Some(" HIGH! ".to_string())),
            (None, Some("low".to_string())),
            (None, "max".to_string()),
        );
        assert_eq!(model9, None);
        assert_eq!(effort9.as_deref(), Some("high"));

        let (model10, effort10) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (None, None),
            (None, None),
            (
                Some("gemini-3.8-flash".to_string()),
                Some("h/igh".to_string()),
            ),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model10.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort10, None);

        let (model11, effort11) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (None, None),
            (None, Some("bogus".to_string())),
            (None, Some("low".to_string())),
            (None, "max".to_string()),
        );
        assert_eq!(model11, None);
        assert_eq!(effort11, None);

        let (model12, effort12) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (
                Some("gemini-3.8-flash".to_string()),
                Some("xhigh".to_string()),
            ),
            (
                Some("gemini-3.8-pro".to_string()),
                Some("bogus".to_string()),
            ),
            (None, Some("low".to_string())),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model12.as_deref(), Some("gemini-3.8-pro"));
        assert_eq!(effort12, None);

        let (model13, effort13) = resolve_final_model_and_effort(
            BackendKind::Agy,
            (
                Some("gemini-3.8-flash".to_string()),
                Some("xhigh".to_string()),
            ),
            (None, Some("bogus".to_string())),
            (None, Some("low".to_string())),
            (Some("gemini-3.8-flash".to_string()), "max".to_string()),
        );
        assert_eq!(model13.as_deref(), Some("gemini-3.8-flash"));
        assert_eq!(effort13.as_deref(), Some("xhigh"));
    }
}
