use super::{
	EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_ACTOR_LOGIN,
	EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS, EXTERNAL_REVIEW_PASS_PHRASE,
	ExternalReviewRequestCiGate, Path, PostReviewLaneClassification, PostReviewLaneDecision,
	PostReviewLaneSnapshot, PostReviewLaneStateLoad, PostReviewOrchestrationStatus,
	PostReviewRuntimeState, PrivateExecutionEvent, PullRequestReviewState,
	PullRequestReviewStateInspector, ReviewCheckpointArtifactLookup, ReviewOrchestrationMarker,
	ReviewOrchestrationPhase, Value, WorkflowDocument, blocked_post_review_lane,
	blocked_post_review_lane_from_handoff, blocked_post_review_lane_from_state,
	external_review_request_ci_gate, eyre, failed_checks_require_repair, github,
	merge_state_requires_review_repair, readback_degraded_post_review_lane_from_handoff,
	review_state_clean_path_landing_gates_satisfied, review_state_landing_requires_agent_fallback,
	validate_post_review_lane_worktree, worktree_head_descends_from_review_handoff,
};

pub(in crate::orchestrator) fn apply_pre_orchestration_post_review_classification(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
) -> bool {
	if review_state.state == "MERGED" {
		classification.decision = PostReviewLaneDecision::Continue;
		classification.reason = String::from("pull_request_merged_closeout_pending");

		return true;
	}
	if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state() {
		*classification = blocked_post_review_lane_from_state(
			review_state,
			"issue_completed_before_pull_request_merged",
		);

		return true;
	}
	if review_state.state != "OPEN" {
		*classification =
			blocked_post_review_lane_from_state(review_state, "pull_request_not_open");

		return true;
	}
	if review_state.is_draft {
		*classification =
			blocked_post_review_lane_from_state(review_state, "pull_request_is_draft");

		return true;
	}
	if review_state.unresolved_review_threads > 0 {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("unresolved_review_threads");

		return true;
	}
	if matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED")) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("review_changes_requested");

		return true;
	}
	if failed_checks_require_repair(
		review_state.status_check_rollup_state.as_deref(),
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("required_checks_failed");

		return true;
	}

	if let Some(reason) = merge_state_requires_review_repair(
		&review_state.mergeable,
		&review_state.merge_state_status,
	) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from(reason);

		return true;
	}

	false
}

pub(in crate::orchestrator) fn apply_non_github_review_post_review_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	orchestration_marker: Option<&ReviewOrchestrationMarker>,
	now_unix_epoch: i64,
) -> crate::prelude::Result<()> {
	if let Some(orchestration_marker) = orchestration_marker {
		let phase =
			ReviewOrchestrationPhase::parse(orchestration_marker.phase()).map_err(|error| {
				eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
			})?;

		if phase == ReviewOrchestrationPhase::WaitingForMerge {
			if let Some(auto_merge_enabled_at) =
				orchestration_marker.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"non_github_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("non_github_review_waiting_for_merge");
			}

			return Ok(());
		}
		if phase == ReviewOrchestrationPhase::RepairRequired {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if review_state_landing_requires_agent_fallback(review_state) {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("non_github_review_repair_required")
			};

			return Ok(());
		}
	}

	if review_state_clean_path_landing_gates_satisfied(review_state) {
		classification.decision = PostReviewLaneDecision::ReadyToLand;
		classification.reason = String::from("non_github_review_ready_to_land");
	} else if review_state_landing_requires_agent_fallback(review_state) {
		classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
		classification.reason = String::from("retained_landing_agent_fallback_required");
	} else {
		classification.reason = String::from("non_github_review_waiting_gates");
	}

	Ok(())
}

pub(in crate::orchestrator) fn load_post_review_orchestration_marker(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<Option<ReviewOrchestrationMarker>> {
	let review_handoff = snapshot
		.review_handoff
		.as_ref()
		.expect("review handoff should exist before orchestration classification");
	let orchestration_marker = if let Some(runtime_state) = runtime_state {
		runtime_state.state_store.review_orchestration_marker(
			runtime_state.project_id,
			&snapshot.issue.id,
			review_handoff,
		)?
	} else {
		None
	};
	let Some(orchestration_marker) = orchestration_marker else {
		if clean_current_head_review_repair_writeback_pending(
			snapshot,
			review_state,
			runtime_state,
		)? {
			classification.reason =
				String::from("review_repair_writeback_missing_lifecycle_marker");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_missing_lifecycle_marker"));

			return Ok(None);
		}

		classification.reason = String::from("external_review_request_pending");

		return Ok(None);
	};

	if let Some(reason) =
		validate_review_orchestration_marker(snapshot, review_state, &orchestration_marker)
	{
		if reason == "review_orchestration_head_mismatch"
			&& clean_current_head_review_repair_writeback_pending(
				snapshot,
				review_state,
				runtime_state,
			)? {
			classification.reason = String::from("review_repair_writeback_stale_lifecycle_marker");
			classification.readback_warning =
				Some(String::from("review_repair_writeback_stale_lifecycle_marker"));

			return Ok(None);
		}

		*classification = blocked_post_review_lane_from_state(review_state, reason);

		return Ok(None);
	}

	Ok(Some(orchestration_marker))
}

pub(in crate::orchestrator) fn clean_current_head_review_repair_writeback_pending(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<bool> {
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

pub(in crate::orchestrator) fn review_repair_terminal_finalize_event_matches_snapshot(
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

pub(in crate::orchestrator) fn review_repair_completion_intent_matches_current_head(
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

pub(in crate::orchestrator) fn apply_review_orchestration_phase_classification(
	classification: &mut PostReviewLaneClassification,
	review_state: &PullRequestReviewState,
	orchestration_marker: &ReviewOrchestrationMarker,
	orchestration_status: &PostReviewOrchestrationStatus,
	now_unix_epoch: i64,
) {
	match orchestration_status.phase {
		ReviewOrchestrationPhase::RequestPending => {
			match external_review_request_ci_gate(review_state) {
				ExternalReviewRequestCiGate::Ready => {
					classification.reason = String::from("external_review_request_pending");
				},
				ExternalReviewRequestCiGate::WaitForGreenChecks => {
					classification.reason =
						String::from("external_review_request_waiting_for_green_checks");
				},
				ExternalReviewRequestCiGate::RepairRequired => {
					classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
					classification.reason =
						String::from("external_review_request_ci_red_repair_required");
				},
			}
		},
		ReviewOrchestrationPhase::WaitingForAck => {
			if orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_result_pending");
			} else if request_ack_timed_out(orchestration_marker, now_unix_epoch) {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_ack_timeout",
				);
			} else {
				classification.reason = String::from("external_review_ack_pending");
			}
		},
		ReviewOrchestrationPhase::WaitingForResult => {
			if !orchestration_status.request_acknowledged {
				classification.reason = String::from("external_review_ack_pending");
			} else if external_review_has_actionable_feedback(review_state, orchestration_marker) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else if orchestration_status.review_result_arrived {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			} else {
				classification.reason = String::from("external_review_result_pending");
			}
		},
		ReviewOrchestrationPhase::RepairRequired => {
			classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
			classification.reason = if orchestration_status.landing_requires_agent_fallback {
				String::from("retained_landing_agent_fallback_required")
			} else {
				String::from("external_review_feedback_pending_repair")
			};
		},
		ReviewOrchestrationPhase::PassWaitingForGates => {
			if external_review_has_actionable_feedback(review_state, orchestration_marker) {
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("external_review_feedback_pending_repair");
			} else if orchestration_status.strict_pass
				&& orchestration_status.clean_path_landing_gates_satisfied
			{
				classification.decision = PostReviewLaneDecision::ReadyToLand;
				classification.reason = String::from("external_review_passed_strict");
			} else if orchestration_status.strict_pass
				&& orchestration_status.landing_requires_agent_fallback
			{
				classification.decision = PostReviewLaneDecision::NeedsReviewRepair;
				classification.reason = String::from("retained_landing_agent_fallback_required");
			} else if orchestration_status.strict_pass {
				classification.reason = String::from("external_review_passed_waiting_gates");
			} else {
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_pass_signal_missing",
				);
			}
		},
		ReviewOrchestrationPhase::WaitingForMerge => {
			if let Some(auto_merge_enabled_at) =
				orchestration_marker.auto_merge_enabled_at_unix_epoch()
				&& now_unix_epoch - auto_merge_enabled_at
					> EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
			{
				*classification = blocked_post_review_lane_from_state(
					review_state,
					"external_review_merge_visibility_timeout",
				);
			} else {
				classification.reason = String::from("external_review_waiting_for_merge");
			}
		},
	}
}

pub(in crate::orchestrator) fn load_post_review_lane_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneStateLoad>
where
	I: PullRequestReviewStateInspector,
{
	if let Some(review_handoff) = snapshot.review_handoff.as_ref() {
		let local_head_oid = match validate_post_review_lane_worktree(snapshot, review_handoff) {
			Ok(local_head_oid) => local_head_oid,
			Err(reason) => {
				return Ok(PostReviewLaneStateLoad::Classification(
					blocked_post_review_lane_from_handoff(review_handoff, reason),
				));
			},
		};
		let review_state = match review_state_inspector.inspect_review_state_readback(
			snapshot.worktree.worktree_path(),
			review_handoff.pr_url(),
		) {
			Ok(review_state) => review_state,
			Err(error) => {
				return Ok(PostReviewLaneStateLoad::Classification(
					readback_degraded_post_review_lane_from_handoff(
						review_handoff,
						error.root_cause(),
					),
				));
			},
		};

		return Ok(validate_post_review_lane_review_state(
			review_state,
			snapshot.worktree.branch_name(),
			local_head_oid,
			snapshot.worktree.worktree_path(),
		));
	}

	Ok(PostReviewLaneStateLoad::Classification(blocked_post_review_lane(
		"missing_review_handoff_record",
	)))
}

pub(in crate::orchestrator) fn validate_post_review_lane_review_state(
	review_state: PullRequestReviewState,
	expected_branch_name: &str,
	local_head_oid: &str,
	worktree_path: &Path,
) -> PostReviewLaneStateLoad {
	let Some(pr_owner) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.owner)
	else {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_repository_parse_failed",
		));
	};
	let Some(pr_repo) =
		github::parse_pull_request_url(&review_state.url).ok().map(|locator| locator.repo)
	else {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_repository_parse_failed",
		));
	};

	if review_state.head_repository_owner.as_deref() != Some(pr_owner.as_str()) {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_repository_owner_mismatch",
		));
	}
	if review_state.head_repository_name.as_deref() != Some(pr_repo.as_str()) {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_repository_name_mismatch",
		));
	}
	if review_state.head_ref_name != expected_branch_name {
		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_branch_mismatch",
		));
	}
	if review_state.head_ref_oid != local_head_oid {
		match merged_pr_local_head_matches_landed_lineage(
			worktree_path,
			&review_state,
			local_head_oid,
		) {
			Ok(true) => return PostReviewLaneStateLoad::ReviewState(review_state),
			Ok(false) => {},
			Err(reason) => {
				return PostReviewLaneStateLoad::Classification(
					blocked_post_review_lane_from_state(&review_state, reason),
				);
			},
		}

		return PostReviewLaneStateLoad::Classification(blocked_post_review_lane_from_state(
			&review_state,
			"pull_request_head_mismatch",
		));
	}

	PostReviewLaneStateLoad::ReviewState(review_state)
}

pub(in crate::orchestrator) fn merged_pr_local_head_matches_landed_lineage(
	worktree_path: &Path,
	review_state: &PullRequestReviewState,
	local_head_oid: &str,
) -> std::result::Result<bool, &'static str> {
	if review_state.state != "MERGED" {
		return Ok(false);
	}

	let Some(merge_commit_oid) = review_state.merge_commit_oid.as_deref() else {
		return Ok(false);
	};

	if merge_commit_oid == local_head_oid {
		return Ok(true);
	}

	worktree_head_descends_from_review_handoff(worktree_path, merge_commit_oid, local_head_oid)
		.map_err(|()| "pull_request_merge_commit_lineage_check_failed")
}

pub(in crate::orchestrator) fn validate_review_orchestration_marker(
	snapshot: &PostReviewLaneSnapshot,
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> Option<&'static str> {
	let Some(local_head_oid) = snapshot.local_head_oid.as_deref() else {
		return Some("worktree_head_missing");
	};

	if marker.branch_name() != snapshot.worktree.branch_name() {
		return Some("review_orchestration_branch_mismatch");
	}
	if marker.pr_url() != review_state.url {
		return Some("review_orchestration_pr_mismatch");
	}
	if marker.head_sha() != local_head_oid {
		return Some("review_orchestration_head_mismatch");
	}

	None
}

pub(in crate::orchestrator) fn request_comment_has_eyes(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> Option<bool> {
	let request_comment_id = marker.request_comment_database_id()?;

	Some(
		review_state
			.issue_comments
			.iter()
			.find(|comment| comment.database_id == request_comment_id)
			.is_some_and(|comment| comment.external_review_eyes_reaction_count > 0),
	)
}

pub(in crate::orchestrator) fn request_ack_timed_out(
	marker: &ReviewOrchestrationMarker,
	now_unix_epoch: i64,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	now_unix_epoch - request_created_at_unix_epoch > EXTERNAL_REVIEW_ACK_TIMEOUT_SECS
		&& marker.request_retry_count() >= 1
}

pub(in crate::orchestrator) fn external_review_result_arrived(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
	})
}

pub(in crate::orchestrator) fn external_review_has_strict_pass_signals(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};
	let pass_phrase_seen_after_request = review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_is_strict_pass_signal(&comment.body)
	});

	pass_phrase_seen_after_request
		&& review_state.issue_description_external_review_thumbs_up_count > 0
}

pub(in crate::orchestrator) fn external_review_has_actionable_feedback(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	let Some(request_created_at_unix_epoch) = marker.request_created_at_unix_epoch() else {
		return false;
	};

	review_state.reviews.iter().any(|review| {
		review.submitted_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(review.author_login.as_deref())
			&& matches!(review.state.as_str(), "COMMENTED" | "CHANGES_REQUESTED")
			&& external_review_body_has_actionable_feedback(&review.body)
	}) || review_state.issue_comments.iter().any(|comment| {
		Some(comment.database_id) != marker.request_comment_database_id()
			&& comment.created_at_unix_epoch >= request_created_at_unix_epoch
			&& is_external_review_actor_login(comment.author_login.as_deref())
			&& external_review_body_has_actionable_feedback(&comment.body)
	})
}

pub(in crate::orchestrator) fn is_external_review_actor_login(login: Option<&str>) -> bool {
	login.is_some_and(|login| login.eq_ignore_ascii_case(EXTERNAL_REVIEW_ACTOR_LOGIN))
}

pub(in crate::orchestrator) fn external_review_body_is_strict_pass_signal(body: &str) -> bool {
	body.trim() == EXTERNAL_REVIEW_PASS_PHRASE
}

pub(in crate::orchestrator) fn external_review_body_has_actionable_feedback(body: &str) -> bool {
	let trimmed = body.trim();

	!trimmed.is_empty() && !external_review_body_is_strict_pass_signal(trimmed)
}
