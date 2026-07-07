use crate::{
	orchestrator::status::{
		self, PostReviewLaneClassification, PostReviewLaneSnapshot, PostReviewRuntimeState,
		PrivateExecutionEvent, PullRequestReviewState, ReviewCheckpointArtifactLookup, Value,
	},
	prelude::Result,
	state::{ReviewLifecycleReadback, ReviewLifecycleRecord},
};

pub(crate) fn load_post_review_lifecycle_record(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> Result<Option<ReviewLifecycleRecord>> {
	let lifecycle_record = if let Some(runtime_state) = runtime_state {
		runtime_state.state_store.review_lifecycle_record(
			runtime_state.project_id,
			&snapshot.issue.id,
			snapshot.worktree.branch_name(),
		)?
	} else {
		None
	};
	let Some(lifecycle_record) = lifecycle_record else {
		if clean_current_head_review_repair_writeback_pending(
			snapshot,
			review_state,
			runtime_state,
		)? {
			classification.reason =
				String::from("review_repair_writeback_missing_lifecycle_authority");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_missing_lifecycle_authority"));

			return Ok(None);
		}

		classification.reason = String::from("external_review_request_pending");

		return Ok(None);
	};

	if let Some(reason) =
		validate_post_review_lifecycle_record(snapshot, review_state, &lifecycle_record)
	{
		if reason == "review_lifecycle_authority_head_mismatch"
			&& clean_current_head_review_repair_writeback_pending(
				snapshot,
				review_state,
				runtime_state,
			)? {
			classification.reason =
				String::from("review_repair_writeback_stale_lifecycle_authority");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_stale_lifecycle_authority"));

			return Ok(None);
		}

		*classification = status::blocked_post_review_lane_from_state(review_state, reason);

		return Ok(None);
	}

	Ok(Some(lifecycle_record))
}

pub(crate) fn clean_current_head_review_repair_writeback_pending(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> Result<bool> {
	let Some(runtime_state) = runtime_state else {
		return Ok(false);
	};
	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Ok(false);
	};

	if review_state.head_ref_oid != local_head_oid
		|| review_state.head_ref_name != snapshot.worktree.branch_name()
	{
		return Ok(false);
	}

	let events = runtime_state
		.state_store
		.list_private_execution_events_for_issue(runtime_state.project_id, &snapshot.issue.id)?;

	for terminal_event in events.iter().rev() {
		if !review_repair_terminal_finalize_event_matches_snapshot(terminal_event, snapshot) {
			continue;
		}

		let Some(intent_event) = events.iter().rev().find(|event| {
			event.run_id() == terminal_event.run_id()
				&& event.attempt_number() == terminal_event.attempt_number()
				&& review_repair_completion_intent_matches_current_head(
					event,
					snapshot,
					review_state,
					local_head_oid,
				)
		}) else {
			continue;
		};
		let Some(checkpoint) = runtime_state.state_store.review_checkpoint_artifact(
			ReviewCheckpointArtifactLookup {
				project_id: runtime_state.project_id,
				issue_id: &snapshot.issue.id,
				phase: "repair",
				review_level: runtime_state.review_level.as_str(),
				head_sha: local_head_oid,
			},
		)?
		else {
			continue;
		};

		if checkpoint.status() == "clean"
			&& checkpoint.head_sha() == local_head_oid
			&& checkpoint.run_id() == intent_event.run_id()
			&& checkpoint.attempt_number() == intent_event.attempt_number()
		{
			return Ok(true);
		}
	}

	Ok(false)
}

pub(crate) fn review_repair_terminal_finalize_event_matches_snapshot(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
) -> bool {
	let payload = event.payload();

	event.event_type() == "terminal_finalize"
		&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
		&& payload.get("mode").and_then(Value::as_str) == Some("repair")
		&& payload.get("branch").and_then(Value::as_str) == Some(snapshot.worktree.branch_name())
		&& payload.get("worktree_path").and_then(Value::as_str)
			== Some(snapshot.worktree.worktree_path().display().to_string().as_str())
}

pub(crate) fn review_repair_completion_intent_matches_current_head(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	local_head_oid: &str,
) -> bool {
	let payload = event.payload();

	event.event_type() == "review_completion_intent"
		&& payload.get("path").and_then(Value::as_str) == Some("review_repair")
		&& payload.get("mode").and_then(Value::as_str) == Some("repair")
		&& payload.get("branch").and_then(Value::as_str) == Some(snapshot.worktree.branch_name())
		&& payload.get("worktree_path").and_then(Value::as_str)
			== Some(snapshot.worktree.worktree_path().display().to_string().as_str())
		&& payload.get("pr_url").and_then(Value::as_str) == Some(review_state.url.as_str())
		&& payload.get("pr_head_ref").and_then(Value::as_str)
			== Some(review_state.head_ref_name.as_str())
		&& payload.get("pr_head_oid").and_then(Value::as_str) == Some(local_head_oid)
}

pub(crate) fn validate_post_review_lifecycle_record(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	marker: &impl ReviewLifecycleReadback,
) -> Option<&'static str> {
	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Some("worktree_head_missing");
	};

	if marker.branch_name() != snapshot.worktree.branch_name() {
		return Some("review_lifecycle_authority_branch_mismatch");
	}
	if marker.pr_url() != review_state.url {
		return Some("review_lifecycle_authority_pr_mismatch");
	}
	if marker.head_sha() != local_head_oid {
		return Some("review_lifecycle_authority_head_mismatch");
	}

	None
}
