use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn strict_failure_no_external_restart() {
	landing_fallbacks::assert_strict_failure_no_external_restart();
}
