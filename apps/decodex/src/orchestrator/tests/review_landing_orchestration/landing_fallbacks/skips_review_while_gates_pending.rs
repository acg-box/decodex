use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn skips_review_while_gates_pending() {
	landing_fallbacks::assert_skips_review_while_gates_pending();
}
