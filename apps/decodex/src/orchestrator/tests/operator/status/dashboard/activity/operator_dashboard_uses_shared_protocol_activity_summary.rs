use crate::orchestrator::tests::operator::status::dashboard;

#[test]
fn operator_dashboard_uses_shared_protocol_activity_summary() {
	let response = dashboard::dashboard_response();

	assert!(response.contains("function protocolActivity(run)"));
	assert!(response.contains("function protocolActivityFocus(run)"));
	assert!(response.contains("function protocolActivityRecentSummary(run)"));
	assert!(response.contains("function protocolActivityDebugSummary(run)"));
	assert!(!response.contains("function normalizedProtocolRateLimitStatus(value)"));
	assert!(!response.contains("status.includes(\"/\") || status.includes(\" \")"));
	assert!(!response.contains("protocolActivityRateLimitDisplay(run, \"\")"));
	assert!(!response.contains("parts.splice(2, 0, `rate limit ${rateLimit}`);"));
	assert!(!response.contains("`rate ${protocolActivityRateLimitDisplay(run)}`"));
	assert!(response.contains("facts.push([\"focus\", detailLabel(focus)]);"));
	assert!(response.contains("return \"approval/user input\";"));
	assert!(response.contains("return \"protocol idleness\";"));
	assert!(response.contains("field(\"Protocol activity\", protocolActivityDebugSummary(run))"));
	assert!(!response.contains("field(\"Rate limit\", protocolActivityRateLimitDisplay(run))"));
}
