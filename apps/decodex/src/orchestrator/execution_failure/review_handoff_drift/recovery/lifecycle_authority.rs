use crate::{
	orchestrator::{
		execution_failure::{
			IssueRunPlan, Result, ServiceConfig, StateStore,
			review_handoff_drift::{command, types::REVIEW_HANDOFF_REBOUND_LIFECYCLE_PHASE},
		},
		kernel::command::CommandIntentKind,
	},
	state::{ReviewLifecycleRecord, ReviewLifecycleTransitionInput},
};

pub(in crate::orchestrator::execution_failure::review_handoff_drift::recovery) fn rebound_review_handoff_lifecycle_authority(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	lifecycle_record: &ReviewLifecycleRecord,
	local_head_sha: &str,
) -> Result<bool> {
	let rebounded_authority = lifecycle_record.head_sha() != local_head_sha
		|| lifecycle_record.phase() != REVIEW_HANDOFF_REBOUND_LIFECYCLE_PHASE;

	command::review_handoff_drift_command_adapter(
		command::review_handoff_drift_lifecycle_authority_rebind_command_intent(
			&issue_run.issue.id,
			lifecycle_record.run_id(),
		),
		CommandIntentKind::SyncReviewLifecycleAuthority,
	)?;

	state_store.record_review_lifecycle_transition(
		project.service_id(),
		&issue_run.issue.id,
		ReviewLifecycleTransitionInput {
			run_id: lifecycle_record.run_id(),
			attempt_number: lifecycle_record.attempt_number(),
			branch_name: lifecycle_record.branch_name(),
			pr_url: lifecycle_record.pr_url(),
			head_sha: local_head_sha,
			phase: REVIEW_HANDOFF_REBOUND_LIFECYCLE_PHASE,
			request_comment_database_id: None,
			request_created_at_unix_epoch: None,
			request_description_thumbs_up_count: None,
			request_retry_count: 0,
			external_round_count: lifecycle_record.external_round_count(),
			auto_merge_enabled_at_unix_epoch: None,
		},
	)?;

	Ok(rebounded_authority)
}
