use crate::orchestrator::retained_review_orchestration::{
	self, CommandIntent, CommandIntentKind, Result, RetainedReviewLane,
	RetainedReviewOrchestrationMarkerFields, ReviewHandoffMarker, ReviewOrchestrationMarker,
	ReviewOrchestrationPhase, StateStore, TrackerIssue, eyre,
};

pub(super) fn write_retained_review_orchestration_marker_for_command(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	kind: CommandIntentKind,
	reason: &str,
	phase: ReviewOrchestrationPhase,
	fields: RetainedReviewOrchestrationMarkerFields,
) -> Result<()> {
	write_retained_review_orchestration_marker(
		state_store,
		lane,
		retained_review_orchestration::retained_review_command_intent(lane, kind, reason),
		kind,
		phase,
		fields,
	)
}

pub(crate) fn ensure_review_orchestration_marker(
	project_id: &str,
	state_store: &StateStore,
	issue: &TrackerIssue,
	review_handoff: &ReviewHandoffMarker,
	local_head_oid: &str,
) -> Result<ReviewOrchestrationMarker> {
	if let Some(marker) =
		state_store.review_orchestration_marker(project_id, &issue.id, review_handoff)?
	{
		if marker.branch_name() == review_handoff.branch_name()
			&& marker.pr_url() == review_handoff.pr_url()
			&& marker.head_sha() != local_head_oid
		{
			let rebound_marker = ReviewOrchestrationMarker::new(
				marker.run_id().to_owned(),
				marker.attempt_number(),
				review_handoff.branch_name().to_owned(),
				review_handoff.pr_url().to_owned(),
				local_head_oid.to_owned(),
				ReviewOrchestrationPhase::RequestPending.as_str(),
				None,
				None,
				None,
				0,
				marker.external_round_count(),
				None,
			);

			retained_review_orchestration::retained_review_command_adapter(
				retained_review_orchestration::retained_review_command_intent_for_issue(
					&issue.id,
					Some(marker.run_id()),
					CommandIntentKind::SyncReviewOrchestrationMarker,
					"review_orchestration_marker_rebound",
				),
				CommandIntentKind::SyncReviewOrchestrationMarker,
			)?;

			state_store.upsert_review_orchestration_marker(
				project_id,
				&issue.id,
				&rebound_marker,
			)?;

			tracing::info!(
				service_id = project_id,
				issue_id = issue.id.as_str(),
				branch = review_handoff.branch_name(),
				pr_url = review_handoff.pr_url(),
				old_head_sha = marker.head_sha(),
				new_head_sha = local_head_oid,
				"Rebound stale retained review orchestration marker to current PR head."
			);

			return Ok(rebound_marker);
		}

		return Ok(marker);
	}

	let marker = ReviewOrchestrationMarker::new(
		review_handoff.run_id().to_owned(),
		review_handoff.attempt_number(),
		review_handoff.branch_name().to_owned(),
		review_handoff.pr_url().to_owned(),
		local_head_oid.to_owned(),
		ReviewOrchestrationPhase::RequestPending.as_str(),
		None,
		None,
		None,
		0,
		0,
		None,
	);

	retained_review_orchestration::retained_review_command_adapter(
		retained_review_orchestration::retained_review_command_intent_for_issue(
			&issue.id,
			Some(review_handoff.run_id()),
			CommandIntentKind::SyncReviewOrchestrationMarker,
			"review_orchestration_marker_created",
		),
		CommandIntentKind::SyncReviewOrchestrationMarker,
	)?;

	state_store.upsert_review_orchestration_marker(project_id, &issue.id, &marker)?;

	Ok(marker)
}

fn write_retained_review_orchestration_marker(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	command_intent: CommandIntent,
	expected_kind: CommandIntentKind,
	phase: ReviewOrchestrationPhase,
	fields: RetainedReviewOrchestrationMarkerFields,
) -> Result<()> {
	retained_review_orchestration::retained_review_command_adapter(command_intent, expected_kind)?;

	let local_head_oid =
		lane.snapshot.local_head_oid.as_deref().ok_or_else(|| {
			eyre::eyre!("Retained review orchestration requires a local lane HEAD.")
		})?;
	let marker = ReviewOrchestrationMarker::new(
		lane.orchestration_marker.run_id().to_owned(),
		lane.orchestration_marker.attempt_number(),
		lane.snapshot.worktree.branch_name().to_owned(),
		lane.review_state.url.clone(),
		local_head_oid.to_owned(),
		phase.as_str(),
		fields.request_comment_database_id,
		fields.request_created_at_unix_epoch,
		None,
		fields.request_retry_count,
		fields.external_round_count,
		fields.auto_merge_enabled_at_unix_epoch,
	);

	state_store.upsert_review_orchestration_marker(
		lane.snapshot.worktree.project_id(),
		&lane.snapshot.issue.id,
		&marker,
	)?;

	Ok(())
}
