use std::path::Path;

use crate::{
	prelude::Result,
	state::{
		RUN_CONTROL_ACTION_ACCEPTED, RUN_CONTROL_ACTION_COMPLETED, RUN_CONTROL_ACTION_FAILED,
		RUN_CONTROL_ACTION_FALLBACK, RUN_CONTROL_ACTION_REJECTED, RUN_CONTROL_ACTION_TIMED_OUT,
		RUN_CONTROL_CHANNEL_STATUS_ACTIVE, RUN_CONTROL_CHANNEL_STATUS_COMPLETED,
		RUN_CONTROL_CHANNEL_STATUS_FAILED, RunControlActionRequest, eyre,
	},
};

pub(in crate::state::store_run_control) fn validate_run_control_channel_inputs(
	run_id: &str,
	attempt_number: i64,
	channel_path: &Path,
	transport: &str,
) -> Result<()> {
	validate_required_run_control_field("run_id", run_id)?;
	validate_required_run_control_field("transport", transport)?;

	if attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}
	if channel_path.as_os_str().is_empty() {
		eyre::bail!("run-control channel_path must not be empty");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::state::store_run_control) fn validate_run_control_action_request(
	request: &RunControlActionRequest<'_>,
) -> Result<()> {
	validate_required_run_control_field("project_id", request.project_id)?;
	validate_required_run_control_field("issue_id", request.issue_id)?;
	validate_required_run_control_field("run_id", request.run_id)?;
	validate_required_run_control_field("source", request.source)?;
	validate_required_run_control_field("action", request.action)?;

	if request.attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}

	if let Some(timeout_ms) = request.timeout_ms
		&& timeout_ms < 0
	{
		eyre::bail!("run-control timeout_ms must not be negative");
	}

	Ok(())
}

pub(in crate::state::store_run_control) fn validate_required_run_control_field(
	name: &str,
	value: &str,
) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("run-control {name} must not be empty");
	}

	Ok(())
}

pub(in crate::state::store_run_control) fn validate_run_control_channel_status(
	status: &str,
) -> Result<()> {
	if !matches!(
		status,
		RUN_CONTROL_CHANNEL_STATUS_ACTIVE
			| RUN_CONTROL_CHANNEL_STATUS_COMPLETED
			| RUN_CONTROL_CHANNEL_STATUS_FAILED
	) {
		eyre::bail!("unsupported run-control channel status `{status}`");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(in crate::state::store_run_control) fn validate_run_control_action_outcome(
	outcome: &str,
) -> Result<()> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_ACCEPTED
			| RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_COMPLETED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		eyre::bail!("unsupported run-control action outcome `{outcome}`");
	}

	Ok(())
}

pub(in crate::state::store_run_control) fn run_control_action_failure_class(
	action: &str,
	outcome: &str,
	reason: &str,
) -> Option<&'static str> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		return None;
	}
	if action == "steer" && reason == "turn_mismatch" {
		return Some("stale_expected_turn_id");
	}
	if action == "steer" && reason == "active_turn_not_steerable" {
		return Some("active_turn_not_steerable");
	}
	if action == "steer" && reason == "app_server_turn_steer_unsupported" {
		return Some("app_server_turn_steer_unsupported");
	}

	Some("run_control_action_failed")
}
