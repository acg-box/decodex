use crate::{
	agent::app_server::preflight::{
		AppServerCapabilityPreflightReport, PREFLIGHT_EVENT_TYPE, RunRecorder, serde_json,
	},
	prelude::Result,
};

pub(crate) fn record_app_server_preflight_report(
	recorder: &mut RunRecorder<'_>,
	report: &AppServerCapabilityPreflightReport,
) -> Result<()> {
	recorder.record(PREFLIGHT_EVENT_TYPE, &serde_json::to_string(report)?)
}
