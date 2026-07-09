use serde_json::{self, Value};

use crate::{
	autonomy_signal::{AutonomySignal, AutonomySignalKind},
	mcp::{
		self, McpServer, TOOL_AUTONOMY_SUBMIT_SIGNAL, planning,
		planning::autonomy::{
			args::{AutonomySignalInputArgs, AutonomySubmitSignalToolArgs},
			results,
		},
	},
};

impl McpServer {
	pub(in crate::mcp) fn call_autonomy_submit_signal_tool(&self, arguments: Value) -> Value {
		let params = match serde_json::from_value::<AutonomySubmitSignalToolArgs>(arguments) {
			Ok(params) => params,
			Err(_) => {
				return mcp::invalid_tool_arguments(
					TOOL_AUTONOMY_SUBMIT_SIGNAL,
					"`kind`, `signal`, and optional `mode` are required.",
				);
			},
		};
		let mode = match planning::planning_mode(
			params.mode.as_deref(),
			"dry_run",
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
		) {
			Ok(mode) => mode,
			Err(result) => return result,
		};
		let project_id = match planning::planning_project_id(
			&self.context,
			params.project_id.as_deref(),
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
		) {
			Ok(project_id) => project_id,
			Err(result) => return result,
		};
		let signal = match autonomy_signal_from_tool_args(params.kind, params.signal, &project_id) {
			Ok(signal) => signal,
			Err(result) => return result,
		};

		if mode == "apply" && !planning::planning_authority_present(params.authority.as_ref()) {
			return planning::missing_authority_refusal(
				TOOL_AUTONOMY_SUBMIT_SIGNAL,
				"autonomy_submit_signal apply requires authority.source and authority.reason.",
			);
		}
		if mode == "dry_run" {
			return mcp::tool_success(results::autonomy_signal_tool_result(
				&project_id,
				&signal,
				mode,
				false,
				None,
			));
		}

		let store = match planning::planning_state_store(&self.context, TOOL_AUTONOMY_SUBMIT_SIGNAL)
		{
			Ok(store) => store,
			Err(result) => return result,
		};

		match store.record_autonomy_signal(&project_id, signal) {
			Ok(record) => mcp::tool_success(results::autonomy_signal_tool_result(
				&project_id,
				record.signal(),
				mode,
				true,
				Some(record.updated_at()),
			)),
			Err(error) => mcp::tool_refusal(
				"autonomy_signal_refused",
				format!("Autonomy signal was refused by Decodex authority checks: {error}"),
			),
		}
	}
}

fn autonomy_signal_from_tool_args(
	kind: AutonomySignalKind,
	input: AutonomySignalInputArgs,
	project_id: &str,
) -> Result<AutonomySignal, Value> {
	let input = input.into_signal_input(project_id);
	let signal = match kind {
		AutonomySignalKind::RuntimeHealth => AutonomySignal::runtime_health(input),
		AutonomySignalKind::ValidationRegression => AutonomySignal::validation_regression(input),
		AutonomySignalKind::ReviewFeedbackCluster => AutonomySignal::review_feedback_cluster(input),
		AutonomySignalKind::UserFeedbackCluster => AutonomySignal::user_feedback_cluster(input),
		AutonomySignalKind::SpecDrift => AutonomySignal::spec_drift(input),
		AutonomySignalKind::ProtocolDrift => AutonomySignal::protocol_drift(input),
		AutonomySignalKind::MetricRegression => AutonomySignal::metric_regression(input),
		AutonomySignalKind::ExecutionFriction => AutonomySignal::execution_friction(input),
		AutonomySignalKind::OpenWikiDrift => AutonomySignal::openwiki_drift(input),
	};

	signal.map_err(|error| {
		mcp::tool_refusal(
			"autonomy_signal_refused",
			format!("Autonomy signal did not satisfy Decodex signal requirements: {error}"),
		)
	})
}
