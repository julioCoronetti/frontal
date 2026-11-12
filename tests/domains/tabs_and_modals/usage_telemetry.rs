use frontal::protocol::TokenUsage;
use frontal::ui::modal::usage::UsageState;
use frontal::ui::modal::UsageModal;

#[test]
fn test_usage_modal_for_opencode_context_and_cache_telemetry() {
    let last = TokenUsage {
        input_tokens: 32_000,
        cache_read_tokens: 32_000,
        ..Default::default()
    };
    let acc = TokenUsage {
        input_tokens: 64_000,
        cache_read_tokens: 64_000,
        ..Default::default()
    };
    let modal = UsageModal::for_opencode("opencode-go/deepseek-v4.1-flash", Some(last), acc);
    let state = modal.state.lock().unwrap();
    let UsageState::Ready(summary) = &*state else {
        panic!("expected ready telemetry")
    };
    assert_eq!(
        summary.plan.tier_name,
        "OpenCode · opencode-go/deepseek-v4.1-flash"
    );
    assert_eq!(summary.groups[0].name, "Context Window");
    assert_eq!(summary.groups[0].buckets[0].remaining_pct, 50.0);
    assert_eq!(summary.groups[1].buckets[0].remaining_pct, 50.0);
    assert_eq!(summary.groups[2].buckets[0].name, "Accumulated");
}

#[test]
fn test_parse_go_usage_windows_and_summary_mapping() {
    let json = r#"{"usage":{"rolling":{"status":"ok","percent":42,"resetsAt":"2099-01-01T00:00:00.000Z"},"weekly":{"status":"ok","percent":18,"resetsAt":"2099-01-05T00:00:00.000Z"},"monthly":{"status":"ok","percent":100,"resetsAt":"2099-01-20T00:00:00.000Z"}}}"#;
    let usage = frontal::ui::modal::usage::opencode_api::parse_go_usage(json).unwrap();
    assert_eq!(usage.rolling.percent, 42.0);
    assert!(frontal::ui::modal::usage::opencode_api::format_reset_epoch(
        usage.rolling.resets_at_ms
    )
    .starts_with("Refreshes in "));

    let summary = frontal::ui::modal::usage::opencode_api::summary_from_go(&usage);
    assert_eq!(summary.plan.tier_name, "OpenCode Go");
    assert_eq!(summary.groups[0].name, "GO USAGE WINDOWS");
    assert_eq!(summary.groups[0].buckets[0].remaining_pct, 58.0);
    assert_eq!(summary.groups[0].buckets[1].remaining_pct, 82.0);
    assert_eq!(summary.groups[0].buckets[2].remaining_pct, 0.0);
    assert!(summary.groups[0].buckets[2].exhausted);
}

#[test]
fn test_parse_iso_ms_and_invalid_payloads() {
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_iso_ms("1970-01-02T00:00:00Z"),
        Some(86_400_000)
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_iso_ms("not-a-date"),
        None
    );
    assert!(frontal::ui::modal::usage::opencode_api::parse_go_usage("{}").is_none());
}

#[test]
fn test_parse_opencode_credential_value() {
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value(
            r#"{"type":"key","key":"oc_test_value"}"#
        )
        .as_deref(),
        Some("oc_test_value")
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value("{}"),
        None
    );
    assert_eq!(
        frontal::ui::modal::usage::opencode_api::parse_credential_value("not json"),
        None
    );
}
