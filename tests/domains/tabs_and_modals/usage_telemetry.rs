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
