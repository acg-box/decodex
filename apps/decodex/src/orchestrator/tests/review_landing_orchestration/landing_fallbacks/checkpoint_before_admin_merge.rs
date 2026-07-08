use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn checkpoint_before_admin_merge() {
	landing_fallbacks::assert_checkpoint_before_admin_merge();
}
