#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::{
	env,
	io::Write as _,
	path::Path,
	process::{Child, Command, Stdio},
};

use crate::{
	cli::AttemptRequest,
	orchestrator::{RunSummary, SpawnRunOnceChildRequest, StateStore, WorkflowDocument},
	prelude::{Result, eyre},
};

pub(super) fn spawn_planned_daemon_child(
	config_path: &Path,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	summary: &RunSummary,
	retry_budget_base: i64,
) -> Result<Child> {
	let issue_claim_handoff =
		Some(state_store.clone_issue_claim_for_child(&summary.issue_id).inspect_err(|_error| {
			let _ = state_store.update_run_status(&summary.run_id, "failed");
			let _ = state_store.clear_lease(&summary.issue_id);
		})?);
	let (dispatch_slot_handoff_file, dispatch_slot_index) =
		state_store.clone_dispatch_slot_for_child(&summary.issue_id)?;
	let dispatch_slot_handoff = Some(dispatch_slot_handoff_file);
	let dispatch_slot_index_handoff = Some(dispatch_slot_index);
	let mut child = spawn_run_once_child(SpawnRunOnceChildRequest {
		config_path,
		preferred_issue_id: summary.issue_id.as_str(),
		preferred_issue_state: summary.issue_state.as_str(),
		preferred_initial_issue_state: Some(summary.initial_issue_state.as_str()),
		dispatch_mode: summary.dispatch_mode,
		preferred_run_id: summary.run_id.as_str(),
		preferred_attempt_number: summary.attempt_number,
		preferred_retry_budget_base: retry_budget_base,
		workflow,
		issue_claim_handoff: issue_claim_handoff.as_ref(),
		dispatch_slot_handoff: dispatch_slot_handoff.as_ref(),
		dispatch_slot_index_handoff,
	})
	.inspect_err(|_error| {
		let _ = state_store.update_run_status(&summary.run_id, "failed");
		let _ = state_store.clear_lease(&summary.issue_id);
	})?;

	state_store.release_handed_off_guards(&summary.issue_id).inspect_err(|_error| {
		let _ = child.kill();
		let _ = child.wait();
		let _ = state_store.update_run_status(&summary.run_id, "failed");
		let _ = state_store.clear_lease(&summary.issue_id);
	})?;

	Ok(child)
}

pub(crate) fn spawn_run_once_child(request: SpawnRunOnceChildRequest<'_>) -> Result<Child> {
	let executable = env::current_exe()?;
	let lease_preacquired =
		request.issue_claim_handoff.is_some() || request.dispatch_slot_handoff.is_some();
	let attempt_request = AttemptRequest {
		dry_run: false,
		issue_id: String::from(request.preferred_issue_id),
		issue_state: String::from(request.preferred_issue_state),
		initial_issue_state: request.preferred_initial_issue_state.map(String::from),
		lease_preacquired,
		#[cfg(unix)]
		issue_claim_fd: request.issue_claim_handoff.map(AsRawFd::as_raw_fd),
		#[cfg(not(unix))]
		issue_claim_fd: None,
		#[cfg(unix)]
		dispatch_slot_fd: request.dispatch_slot_handoff.map(AsRawFd::as_raw_fd),
		#[cfg(not(unix))]
		dispatch_slot_fd: None,
		dispatch_slot_index: request.dispatch_slot_index_handoff,
		dispatch_mode: request.dispatch_mode.into(),
		run_id: String::from(request.preferred_run_id),
		attempt_number: request.preferred_attempt_number,
		retry_budget_base: request.preferred_retry_budget_base,
		workflow_snapshot: request.workflow.to_markdown()?,
	};
	let payload = serde_json::to_vec(&attempt_request)?;
	let mut command = Command::new(executable);

	command
		.args(["_attempt", "--config"])
		.arg(request.config_path)
		.arg("-")
		.stdin(Stdio::piped())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());

	let mut child = command.spawn()?;
	let Some(mut stdin) = child.stdin.take() else {
		let _ = child.kill();
		let _ = child.wait();

		eyre::bail!("Spawned `_attempt` child without a writable stdin handle.");
	};

	if let Err(error) = stdin.write_all(&payload) {
		let _ = child.kill();
		let _ = child.wait();

		eyre::bail!("Failed to write `_attempt` request payload: {error}");
	}

	Ok(child)
}
