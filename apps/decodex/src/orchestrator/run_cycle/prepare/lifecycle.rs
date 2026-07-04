use crate::orchestrator::run_cycle::{Result, StateStore};

pub(in crate::orchestrator::run_cycle::prepare) fn clear_prepare_issue_run_lease(
	state_store: &StateStore,
	dry_run: bool,
	issue_id: &str,
) -> Result<()> {
	if !dry_run {
		state_store.clear_lease(issue_id)?;
	}

	Ok(())
}

pub(in crate::orchestrator::run_cycle::prepare) fn record_starting_attempt(
	state_store: &StateStore,
	run_id: &str,
	issue_id: &str,
	attempt_number: i64,
) -> Result<()> {
	state_store.record_run_attempt(run_id, issue_id, attempt_number, "starting")
}
