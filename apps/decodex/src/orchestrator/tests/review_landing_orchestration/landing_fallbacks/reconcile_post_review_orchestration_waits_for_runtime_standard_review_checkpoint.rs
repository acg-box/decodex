use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn reconcile_post_review_orchestration_waits_for_runtime_standard_review_checkpoint() {
	landing_fallbacks::assert_reconcile_post_review_orchestration_waits_for_runtime_standard_review_checkpoint();
}
