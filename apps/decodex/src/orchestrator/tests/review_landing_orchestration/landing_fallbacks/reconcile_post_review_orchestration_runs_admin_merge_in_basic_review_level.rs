use crate::orchestrator::{ReviewLevel, tests::review_landing_orchestration::landing_fallbacks};

#[test]
fn reconcile_post_review_orchestration_runs_admin_merge_in_basic_review_level() {
	landing_fallbacks::assert_reconcile_post_review_orchestration_runs_admin_merge_without_external_review(
		ReviewLevel::Basic,
	);
}
