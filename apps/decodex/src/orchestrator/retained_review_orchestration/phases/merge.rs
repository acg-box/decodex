use crate::orchestrator::retained_review_orchestration::{
	self, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, IssueTracker,
	PassiveRetainedAttentionRuntime, Result, RetainedReviewLane, ServiceConfig, StateStore,
	WorkflowDocument,
};

pub(in crate::orchestrator::retained_review_orchestration::phases) fn handle_waiting_for_merge_phase<
	T,
>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	now_unix_epoch: i64,
	timeout_reason: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let Some(auto_merge_enabled_at_unix_epoch) =
		lane.lifecycle_record().auto_merge_enabled_at_unix_epoch()
	else {
		return Ok(());
	};

	if now_unix_epoch - auto_merge_enabled_at_unix_epoch
		<= EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
	{
		return Ok(());
	}

	retained_review_orchestration::apply_passive_retained_manual_attention(
		PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		lane.lifecycle_record(),
		timeout_reason,
	)
}
