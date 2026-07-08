use crate::orchestrator::tests::review_landing_orchestration::landing_fallbacks;

#[test]
fn unknown_review_status_attention() {
	landing_fallbacks::assert_unknown_review_status_attention();
}
