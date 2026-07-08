use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn checkpoint_failure_after_budget() {
	landing_fallbacks::assert_checkpoint_failure_after_budget();
}
