use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn waits_for_checkpoint() {
	landing_fallbacks::assert_waits_for_checkpoint();
}
