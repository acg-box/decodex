use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn repair_checkpoint_after_findings() {
	landing_fallbacks::assert_repair_checkpoint_after_findings();
}
