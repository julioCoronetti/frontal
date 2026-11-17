use serde_json::{json, Value};

use crate::transport::EngineConfig;

pub(super) fn collect_setup_requests(
    sid: &str,
    res: &Value,
    config: &EngineConfig,
    served_model: &mut Option<String>,
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

    if let Some(mode) = config.mode.as_deref() {
        if option_supports(res, "mode", mode) && current_option(res, "mode") != Some(mode) {
            out.push((
                "session/set_config_option",
                json!({ "sessionId": sid, "configId": "mode", "value": mode }),
            ));
        }
    }

    out
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
