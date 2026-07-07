use crate::orchestrator::{ReviewLevel, tests::review_landing_orchestration::landing_fallbacks};

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_without_external_review_when_disabled() {
	landing_fallbacks::assert_reconcile_post_review_orchestration_runs_admin_merge_without_external_review(
		ReviewLevel::Off,
	);
}
