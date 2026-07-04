use crate::orchestrator::run_cycle::{
	self, PreferredRunIdentity, Result, StateStore, TrackerIssue,
};

pub(in crate::orchestrator::run_cycle::prepare) fn resolve_prepare_run_identity(
	state_store: &StateStore,
	issue: &TrackerIssue,
	preferred_run_identity: Option<PreferredRunIdentity<'_>>,
) -> Result<Option<(i64, String)>> {
	let next_attempt_number = state_store.next_attempt_number(&issue.id)?;

	match preferred_run_identity {
		Some(preferred_run_identity) => {
			if next_attempt_number > preferred_run_identity.attempt_number {
				let Some(existing_attempt) =
					state_store.run_attempt(preferred_run_identity.run_id)?
				else {
					return Ok(None);
				};

				if existing_attempt.issue_id() != issue.id
					|| existing_attempt.attempt_number() != preferred_run_identity.attempt_number
				{
					return Ok(None);
				}
			}

			Ok(Some((
				preferred_run_identity.attempt_number,
				preferred_run_identity.run_id.to_owned(),
			)))
		},
		None => Ok(Some((
			next_attempt_number,
			run_cycle::build_run_id(&issue.identifier, next_attempt_number)?,
		))),
	}
}
