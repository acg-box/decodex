use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_normalizes_review_state_tokens() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function compactStateToken(value)"));
	assert!(response.contains("return formatDetailToken(value);"));
	assert!(response.contains("function reviewThreadToken(count)"));
	assert!(response.contains(
		"return Number.isFinite(numericCount) && numericCount > 0 ? String(numericCount) : \"NONE\";",
	));
	assert!(response.contains("function optionalCardToken(value)"));
	assert!(response.contains("return token || \"NONE\";"));
	assert!(response.contains("if (/^[A-Z0-9]+$/.test(word) && /[A-Z]/.test(word))"));
	assert!(response.contains(
		"status: lane.mergeable ? `merge ${compactStateToken(lane.mergeable)}` : \"ready\","
	));
	assert!(response.contains(
		"status: lane.check_state ? `checks ${compactStateToken(lane.check_state)}` : \"waiting\","
	));
	assert!(response.contains("`review ${compactStateToken(lane.review_decision)}`"));
	assert!(response.contains("[\"Checks\", compactStateToken(lane.check_state)]"));
	assert!(response.contains("[\"Threads\", reviewThreadToken(lane.unresolved_review_threads)]"));
	assert!(response.contains("[\"Review decision\", compactStateToken(lane.review_decision)]"));
	assert!(response.contains("[\"PR\", optionalCardToken(lane.pr_url)]"));
	assert!(!response.contains("`merge ${displayToken(lane.mergeable)}`"));
	assert!(!response.contains("`checks ${displayToken(lane.check_state)}`"));
	assert!(!response.contains("[\"Checks\", lane.check_state || \"none\"]"));
	assert!(!response.contains("lane.unresolved_review_threads == null ? \"none\""));
	assert!(!response.contains("lane.pr_url || \"none\""));
}
