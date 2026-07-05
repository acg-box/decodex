use crate::orchestrator::tests::operator::status::{
	dashboard,
	dashboard::activity::{self},
};

#[test]
fn operator_dashboard_active_freshness_prefers_live_activity_source() {
	let response = dashboard::dashboard_response();

	activity::assert_dashboard_freshness_source_contract(&response);
	activity::assert_dashboard_lifecycle_activity_contract(&response);
	activity::assert_dashboard_activity_display_regressions(&response);
}
