use crate::{
	orchestrator::retained_review_orchestration::{
		self, CommandIntent, CommandIntentKind, Result, RetainedReviewLane,
		RetainedReviewLifecycleAuthorityFields, StateStore, TrackerIssue, eyre,
	},
	state::{ReviewLifecycleRecord, ReviewLifecycleTransitionInput},
};

pub(super) fn write_retained_review_lifecycle_authority_for_command(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	kind: CommandIntentKind,
	reason: &str,
	phase: &str,
	fields: RetainedReviewLifecycleAuthorityFields,
) -> Result<()> {
	write_retained_review_lifecycle_authority(
		state_store,
		lane,
		retained_review_orchestration::retained_review_command_intent(lane, kind, reason),
		kind,
		phase,
		fields,
	)
}

pub(super) fn write_retained_review_lifecycle_authority_for_current_action(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	kind: CommandIntentKind,
	reason: &str,
	fields: RetainedReviewLifecycleAuthorityFields,
) -> Result<()> {
	write_retained_review_lifecycle_authority(
		state_store,
		lane,
		retained_review_orchestration::retained_review_command_intent(lane, kind, reason),
		kind,
		lane.lifecycle_record().phase(),
		fields,
	)
}

pub(crate) fn ensure_review_lifecycle_authority(
	project_id: &str,
	state_store: &StateStore,
	issue: &TrackerIssue,
	lifecycle_record: &ReviewLifecycleRecord,
	local_head_oid: &str,
) -> Result<ReviewLifecycleRecord> {
	if lifecycle_record.head_sha() != local_head_oid {
		retained_review_orchestration::retained_review_command_adapter(
			retained_review_orchestration::retained_review_command_intent_for_issue(
				&issue.id,
				Some(lifecycle_record.run_id()),
				CommandIntentKind::SyncReviewLifecycleAuthority,
				"review_lifecycle_authority_rebound",
			),
			CommandIntentKind::SyncReviewLifecycleAuthority,
		)?;

		state_store.record_review_lifecycle_transition(
			project_id,
			&issue.id,
			ReviewLifecycleTransitionInput {
				run_id: lifecycle_record.run_id(),
				attempt_number: lifecycle_record.attempt_number(),
				branch_name: lifecycle_record.branch_name(),
				pr_url: lifecycle_record.pr_url(),
				head_sha: local_head_oid,
				phase: "request_pending",
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: lifecycle_record.external_round_count(),
				auto_merge_enabled_at_unix_epoch: None,
			},
		)?;

		tracing::info!(
			service_id = project_id,
			issue_id = issue.id.as_str(),
			branch = lifecycle_record.branch_name(),
			pr_url = lifecycle_record.pr_url(),
			old_head_sha = lifecycle_record.head_sha(),
			new_head_sha = local_head_oid,
			"Rebound stale retained review lifecycle authority to current PR head."
		);

		return reload_review_lifecycle_authority(
			project_id,
			state_store,
			issue,
			lifecycle_record.branch_name(),
		);
	}

	Ok(lifecycle_record.clone())
}

fn write_retained_review_lifecycle_authority(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	command_intent: CommandIntent,
	expected_kind: CommandIntentKind,
	phase: &str,
	fields: RetainedReviewLifecycleAuthorityFields,
) -> Result<()> {
	retained_review_orchestration::retained_review_command_adapter(command_intent, expected_kind)?;

	let local_head_oid =
		lane.snapshot.local_head_oid.as_deref().ok_or_else(|| {
			eyre::eyre!("Retained review orchestration requires a local lane HEAD.")
		})?;
	state_store.record_review_lifecycle_transition(
		lane.snapshot.worktree.project_id(),
		&lane.snapshot.issue.id,
		ReviewLifecycleTransitionInput {
			run_id: lane.lifecycle_record().run_id(),
			attempt_number: lane.lifecycle_record().attempt_number(),
			branch_name: lane.snapshot.worktree.branch_name(),
			pr_url: lane.review_state.url.as_str(),
			head_sha: local_head_oid,
			phase,
			request_comment_database_id: fields.request_comment_database_id,
			request_created_at_unix_epoch: fields.request_created_at_unix_epoch,
			request_description_thumbs_up_count: None,
			request_retry_count: fields.request_retry_count,
			external_round_count: fields.external_round_count,
			auto_merge_enabled_at_unix_epoch: fields.auto_merge_enabled_at_unix_epoch,
		},
	)?;

	Ok(())
}

fn reload_review_lifecycle_authority(
	project_id: &str,
	state_store: &StateStore,
	issue: &TrackerIssue,
	branch_name: &str,
) -> Result<ReviewLifecycleRecord> {
	state_store.review_lifecycle_record(project_id, &issue.id, branch_name)?.ok_or_else(|| {
		eyre::eyre!(
			"Retained review lifecycle authority for `{}` on branch `{}` disappeared after update.",
			issue.identifier,
			branch_name
		)
	})
}
