use crate::orchestrator::{ReviewLevel, tests::review_landing_orchestration::landing_fallbacks};

#[test]
fn admin_merge_without_external_review() {
	landing_fallbacks::assert_admin_merge_without_external_review(ReviewLevel::Off);
}
