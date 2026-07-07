use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn reconcile_post_review_orchestration_routes_blocked_runtime_standard_review_to_attention() {
	landing_fallbacks::assert_reconcile_post_review_orchestration_routes_runtime_standard_review_terminal_status_to_attention(
		"blocked",
		"runtime_standard_review_blocked",
	);
}

#[test]
fn reconcile_post_review_orchestration_routes_architecture_runtime_standard_review_to_attention() {
	landing_fallbacks::assert_reconcile_post_review_orchestration_routes_runtime_standard_review_terminal_status_to_attention(
		"needs_architecture_review",
		"runtime_standard_review_needs_architecture_review",
	);
}
