use crate::orchestrator::run_cycle::{
	self, IssueDispatchMode, IssueTracker, OffsetDateTime, PreferredRunIdentity, Result,
	RetainedReviewRunIdentity, ServiceConfig, StateStore, TargetIssueRunContext, TrackerIssue,
};

pub(crate) fn target_issue_active_claim_blocks_dispatch<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
	issue: &TrackerIssue,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.lease_preacquired {
		return Ok(false);
	}
	if !context.state_store.issue_has_active_shared_claim(context.project.service_id(), issue_id)? {
		return Ok(false);
	}
	if context.dispatch_mode == IssueDispatchMode::Closeout {
		return closeout_lane_active_claim_blocks_dispatch(
			context.project,
			context.state_store,
			issue,
		);
	}

	Ok(true)
}

pub(crate) fn closeout_lane_active_claim_blocks_dispatch(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue: &TrackerIssue,
) -> Result<bool> {
	if !state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(false);
	}

	let Some(lease) = state_store.lease_for_issue(&issue.id)? else {
		return Ok(true);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	run_cycle::retained_closeout_lease_has_fresh_activity(&lease, issue, project, now_unix_epoch)
}

pub(crate) fn target_closeout_preferred_run_identity<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<Option<RetainedReviewRunIdentity>>
where
	T: IssueTracker,
{
	if context.dispatch_mode != IssueDispatchMode::Closeout
		|| context.preferred_run_identity.is_some()
	{
		return Ok(None);
	}

	run_cycle::retained_closeout_preferred_run_identity(
		context.state_store,
		context.project.service_id(),
		issue,
	)
}

pub(crate) fn preferred_run_identity_with_closeout_fallback<'a>(
	preferred_run_identity: Option<PreferredRunIdentity<'a>>,
	closeout_preferred_run_identity: Option<&'a RetainedReviewRunIdentity>,
) -> Option<PreferredRunIdentity<'a>> {
	match (preferred_run_identity, closeout_preferred_run_identity) {
		(Some(identity), _) => Some(identity),
		(None, Some(identity)) => Some(PreferredRunIdentity {
			run_id: identity.run_id.as_str(),
			attempt_number: identity.attempt_number,
		}),
		(None, None) => None,
	}
}

pub(crate) fn target_issue_reuses_existing_closeout_claim<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
	issue: &TrackerIssue,
) -> Result<bool>
where
	T: IssueTracker,
{
	if context.lease_preacquired || context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(false);
	}
	if !context.state_store.issue_has_active_shared_claim(context.project.service_id(), issue_id)? {
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&issue.id)?.is_none() {
		return Ok(false);
	}

	Ok(!closeout_lane_active_claim_blocks_dispatch(context.project, context.state_store, issue)?)
}
