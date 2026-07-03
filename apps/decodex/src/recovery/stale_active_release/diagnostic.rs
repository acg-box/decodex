use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	recovery::{
		context::RecoveryRuntimeMutationPolicy, reports::StaleActiveDiagnostic,
		stale_active_diagnosis,
	},
	state::StateStore,
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

pub(super) fn refreshed_stale_active_release_diagnostic<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	original: &StaleActiveDiagnostic,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut diagnostics = stale_active_diagnosis::diagnose_stale_active_issues(
		config.service_id(),
		workflow,
		config.worktree_root(),
		state_store,
		tracker,
		Some(&original.issue_identifier),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)?;
	let diagnostic = diagnostics.pop().ok_or_else(|| {
		eyre::eyre!("No stale active issue matched `{}`.", original.issue_identifier)
	})?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because safety inspection changed before apply: {}",
			original.issue_identifier,
			diagnostic.blockers.join(", ")
		);
	}
	if diagnostic.issue_id != original.issue_id
		|| diagnostic.latest_run_id != original.latest_run_id
		|| diagnostic.latest_attempt_number != original.latest_attempt_number
	{
		eyre::bail!(
			"`recover stale-active release` refused `{}` because the stale ownership target changed before apply.",
			original.issue_identifier
		);
	}

	Ok(diagnostic)
}
