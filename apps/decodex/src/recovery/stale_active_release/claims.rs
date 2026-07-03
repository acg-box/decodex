use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	recovery::{reports::StaleActiveDiagnostic, stale_active_labels},
	state::StateStore,
};

pub(super) fn clear_stale_active_dead_run_claims_before_release(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<bool> {
	if !diagnostic.run_lease
		|| !diagnostic.evidence.iter().any(|evidence| evidence == "stale_run_lease_present")
	{
		return Ok(false);
	}

	let Some(run_id) = diagnostic.latest_run_id.as_deref() else {
		return Ok(false);
	};
	let mut cleared = false;

	for issue_key in stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic) {
		let Some(lease) = state_store.lease_for_issue(&issue_key)? else {
			continue;
		};

		if lease.project_id() == diagnostic.project_id && lease.run_id() == run_id {
			state_store.clear_lease(&issue_key)?;

			cleared = true;
		}
	}

	Ok(cleared)
}

pub(super) fn ensure_stale_active_run_claim_guard(
	config: &ServiceConfig,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	let issue_keys = stale_active_labels::stale_active_diagnostic_issue_keys(diagnostic);

	match stale_active_labels::stale_active_issue_has_active_shared_claim(
		config.service_id(),
		state_store,
		&issue_keys,
	) {
		Ok(false) => Ok(()),
		Ok(true) => eyre::bail!(
			"`recover stale-active release` refused `{}` because a run lease or shared claim appeared before active-label release.",
			diagnostic.issue_identifier
		),
		Err(error) => eyre::bail!(
			"`recover stale-active release` refused `{}` because run lease/shared claim state could not be inspected before active-label release: {}",
			diagnostic.issue_identifier,
			error
		),
	}
}

pub(super) fn stale_active_attempt_status_needs_terminal_guard(status: &str) -> bool {
	matches!(
		status,
		"starting" | "running" | "continuation_pending" | "stalled" | "failed" | "interrupted"
	)
}
