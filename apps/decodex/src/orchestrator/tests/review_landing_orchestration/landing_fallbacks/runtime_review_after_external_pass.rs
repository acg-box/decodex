use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn runtime_review_after_external_pass() {
	landing_fallbacks::assert_runtime_review_after_external_pass();
}
