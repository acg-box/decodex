use color_eyre::Report;

use crate::agent::{
	app_server::{self, tests::AppServerCapabilityPreflightReport},
	json_rpc::AppServerOutputTimeout,
};

#[test]
fn mcp_preflight_timeout_degrades_to_recorded_ok_check() {
	let error = Report::new(AppServerOutputTimeout);
	let mut report = AppServerCapabilityPreflightReport::new();

	assert!(app_server::mcp_preflight_can_degrade(&error));

	app_server::record_mcp_preflight_degraded(&mut report, &error);

	assert!(!report.has_blockers());
	assert_eq!(report.checks().len(), 1);
	assert_eq!(report.checks()[0].name, "mcp");
	assert_eq!(report.checks()[0].status, app_server::AppServerCapabilityPreflightStatus::Ok);
	assert_eq!(
		report.checks()[0].details.get("degraded_reason").map(String::as_str),
		Some("timeout")
	);
	assert!(report.checks()[0].summary.contains("continuing"));
}
