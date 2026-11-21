use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::mpsc;

use crate::transport::EngineConfig;

pub(super) fn spawn_stdin_writer(mut stdin: ChildStdin) -> mpsc::UnboundedSender<String> {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if stdin.flush().await.is_err() {
                break;
            }
        }
    });
    tx
}

pub(super) fn collect_setup_requests(
    sid: &str,
    res: &Value,
    config: &EngineConfig,
    served_model: &mut Option<String>,
    served_mode: &mut Option<String>,
) -> Vec<(&'static str, Value)> {
    let mut out = Vec::new();
    let current_model = res
        .pointer("/models/currentModelId")
        .and_then(Value::as_str);
    let available_models: Vec<&str> = res
        .pointer("/models/availableModels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("modelId").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();

    let mut applied_effort_via_model = false;
    if let Some(raw_model) = &config.model {
        let sanitized = crate::config::sanitize_model_id(raw_model);
        if !sanitized.is_empty() {
            let stripped = EngineConfig::strip_effort_suffix(&sanitized);
            let combined_candidate = config
                .effort
                .as_deref()
                .map(|e| format!("{}-{}", stripped, e));
            let chosen = combined_candidate
                .as_deref()
                .filter(|c| available_models.contains(c))
                .inspect(|_| applied_effort_via_model = true)
                .or_else(|| {
                    if available_models.contains(&sanitized.as_str()) {
                        Some(sanitized.as_str())
                    } else {
                        None
                    }
                });
            if let Some(model_id) = chosen {
                *served_model = Some(model_id.to_string());
                if current_model != Some(model_id) {
                    out.push((
                        "session/set_model",
                        json!({ "sessionId": sid, "modelId": model_id }),
                    ));
                }
            } else if option_supports(res, "model", stripped)
                && current_option(res, "model") != Some(stripped)
            {
                *served_model = Some(stripped.to_string());
                out.push((
                    "session/set_config_option",
                    json!({ "sessionId": sid, "configId": "model", "value": stripped }),
                ));
            }
        }
    }

    if !applied_effort_via_model {
        if let Some(effort) = config.effort.as_deref() {
            if option_supports(res, "thought_level", effort)
                && current_option(res, "thought_level") != Some(effort)
            {
                out.push((
                    "session/set_config_option",
                    json!({ "sessionId": sid, "configId": "thought_level", "value": effort }),
                ));
            }
        }
    }

    if let Some(raw_mode) = config.mode.as_deref() {
        let mode = crate::config::BackendKind::normalize_mode(raw_mode).unwrap_or(raw_mode);
        let resolved_mode = if option_supports(res, "mode", mode) {
            Some(mode)
        } else if mode == "accept-edits" && option_supports(res, "mode", "default") {
            Some("default")
        } else {
            None
        };
        if let Some(wire_mode) = resolved_mode {
            *served_mode = Some(mode.to_string());
            if current_option(res, "mode") != Some(wire_mode) {
                out.push((
                    "session/set_config_option",
                    json!({ "sessionId": sid, "configId": "mode", "value": wire_mode }),
                ));
            }
        } else {
            *served_mode = Some(current_mode_fallback(res));
        }
    }

    out
}

pub(super) fn current_mode_fallback(res: &Value) -> String {
    current_option(res, "mode")
        .and_then(crate::config::BackendKind::normalize_mode)
        .unwrap_or(crate::config::DEFAULT_MODE)
        .to_string()
}

fn find_config_option<'a>(res: &'a Value, config_id: &str) -> Option<&'a Value> {
    res.get("configOptions")?
        .as_array()?
        .iter()
        .find(|opt| opt.get("id").and_then(Value::as_str) == Some(config_id))
}

fn current_option<'a>(res: &'a Value, config_id: &str) -> Option<&'a str> {
    find_config_option(res, config_id)?
        .get("currentValue")
        .and_then(Value::as_str)
}

fn option_supports(res: &Value, config_id: &str, value: &str) -> bool {
    let Some(opt) = find_config_option(res, config_id) else {
        return false;
    };
    let Some(values) = opt.get("options").and_then(Value::as_array) else {
        return false;
    };
    values
        .iter()
        .any(|entry| entry.get("value").and_then(Value::as_str) == Some(value))
}
