use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn blocked_runtime_review_to_attention() {
	landing_fallbacks::assert_terminal_review_status_attention(
		"blocked",
		"runtime_standard_review_blocked",
	);
}

#[test]
fn architecture_runtime_review_to_attention() {
	landing_fallbacks::assert_terminal_review_status_attention(
		"needs_architecture_review",
		"runtime_standard_review_needs_architecture_review",
	);
}
