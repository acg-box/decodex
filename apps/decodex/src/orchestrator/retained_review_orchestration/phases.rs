#[allow(clippy::wildcard_imports)] use super::*;

pub(super) fn reconcile_retained_review_lane<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	if !project.codex().review_level().uses_github_review() {
		return handle_non_github_review_lane(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			github_token,
			now_unix_epoch,
		);
	}

	let phase =
		ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase()).map_err(|error| {
			eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
		})?;

	match phase {
		ReviewOrchestrationPhase::RequestPending =>
			handle_request_pending_phase(project, state_store, lane, github_token),
		ReviewOrchestrationPhase::WaitingForAck => handle_waiting_for_ack_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			github_token,
			now_unix_epoch,
		),
		ReviewOrchestrationPhase::WaitingForResult
		| ReviewOrchestrationPhase::PassWaitingForGates => {
			let mut runtime = RetainedReviewRuntime {
				tracker,
				project,
				workflow,
				state_store,
				github_token,
				now_unix_epoch,
			};

			handle_waiting_for_result_phase(&mut runtime, lane, phase)
		},
		ReviewOrchestrationPhase::RepairRequired => Ok(()),
		ReviewOrchestrationPhase::WaitingForMerge => handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"external_review_merge_visibility_timeout",
		),
	}
}

fn handle_non_github_review_lane<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let phase =
		ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase()).map_err(|error| {
			eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
		})?;

	if phase == ReviewOrchestrationPhase::WaitingForMerge {
		return handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"non_github_review_merge_visibility_timeout",
		);
	}
	if external_review_requires_repair(&lane.review_state, &lane.orchestration_marker)
		|| failed_checks_require_repair(
			lane.review_state.status_check_rollup_state.as_deref(),
			&lane.review_state.merge_state_status,
		) || merge_state_requires_review_repair(
		&lane.review_state.mergeable,
		&lane.review_state.merge_state_status,
	)
	.is_some()
	{
		return write_retained_review_orchestration_marker(
			state_store,
			lane,
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if review_state_landing_requires_agent_fallback(&lane.review_state) {
		return write_retained_review_orchestration_marker(
			state_store,
			lane,
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if !review_state_landing_gates_satisfied(&lane.review_state) {
		return Ok(());
	}

	let mut runtime = RetainedReviewRuntime {
		tracker,
		project,
		workflow,
		state_store,
		now_unix_epoch,
		github_token,
	};

	start_retained_admin_merge(
		&mut runtime,
		lane,
		RetainedAdminMergeReasons {
			admin_merge_unavailable: "non_github_review_admin_merge_unavailable",
			admin_merge_failed: "non_github_review_admin_merge_failed",
		},
	)
}

fn handle_request_pending_phase(
	project: &ServiceConfig,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
) -> Result<()> {
	match external_review_request_ci_gate(&lane.review_state) {
		ExternalReviewRequestCiGate::Ready => {},
		ExternalReviewRequestCiGate::WaitForGreenChecks => return Ok(()),
		ExternalReviewRequestCiGate::RepairRequired => {
			return write_retained_review_orchestration_marker(
				state_store,
				lane,
				ReviewOrchestrationPhase::RepairRequired,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		},
	}

	let github_token = retained_review_github_token(project, github_token)?;
	let (comment_id, created_at_unix_epoch) = github::post_pull_request_issue_comment(
		lane.snapshot.worktree.worktree_path(),
		lane.review_state.url.as_str(),
		EXTERNAL_REVIEW_REQUEST_BODY,
		github_token,
		project.github().command_path(),
	)?;

	write_retained_review_orchestration_marker(
		state_store,
		lane,
		ReviewOrchestrationPhase::WaitingForAck,
		RetainedReviewOrchestrationMarkerFields {
			request_comment_database_id: Some(comment_id),
			request_created_at_unix_epoch: Some(created_at_unix_epoch),
			request_retry_count: 0,
			..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
		},
	)
}

fn handle_waiting_for_ack_phase<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	if request_comment_has_eyes(&lane.review_state, &lane.orchestration_marker).unwrap_or(false) {
		return write_retained_review_orchestration_marker(
			state_store,
			lane,
			ReviewOrchestrationPhase::WaitingForResult,
			RetainedReviewOrchestrationMarkerFields {
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	let Some(request_created_at_unix_epoch) =
		lane.orchestration_marker.request_created_at_unix_epoch()
	else {
		return Ok(());
	};

	if now_unix_epoch - request_created_at_unix_epoch <= EXTERNAL_REVIEW_ACK_TIMEOUT_SECS {
		return Ok(());
	}
	if lane.orchestration_marker.request_retry_count() == 0 {
		let github_token = retained_review_github_token(project, github_token)?;
		let (comment_id, created_at_unix_epoch) = github::post_pull_request_issue_comment(
			lane.snapshot.worktree.worktree_path(),
			lane.review_state.url.as_str(),
			EXTERNAL_REVIEW_REQUEST_BODY,
			github_token,
			project.github().command_path(),
		)?;

		return write_retained_review_orchestration_marker(
			state_store,
			lane,
			ReviewOrchestrationPhase::WaitingForAck,
			RetainedReviewOrchestrationMarkerFields {
				request_comment_database_id: Some(comment_id),
				request_created_at_unix_epoch: Some(created_at_unix_epoch),
				request_retry_count: 1,
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	apply_passive_retained_manual_attention(
		PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		&lane.orchestration_marker,
		"external_review_ack_timeout",
	)
}

fn handle_waiting_for_result_phase<T>(
	runtime: &mut RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	phase: ReviewOrchestrationPhase,
) -> Result<()>
where
	T: IssueTracker,
{
	if external_review_requires_repair(&lane.review_state, &lane.orchestration_marker) {
		return write_retained_review_orchestration_marker(
			runtime.state_store,
			lane,
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields {
				external_round_count: lane
					.orchestration_marker
					.external_round_count()
					.saturating_add(1),
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}
	if failed_checks_require_repair(
		lane.review_state.status_check_rollup_state.as_deref(),
		&lane.review_state.merge_state_status,
	) || merge_state_requires_review_repair(
		&lane.review_state.mergeable,
		&lane.review_state.merge_state_status,
	)
	.is_some()
	{
		return write_retained_review_orchestration_marker(
			runtime.state_store,
			lane,
			ReviewOrchestrationPhase::RepairRequired,
			RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
		);
	}
	if external_review_has_strict_pass_signals(&lane.review_state, &lane.orchestration_marker) {
		if review_state_clean_path_landing_gates_satisfied(&lane.review_state) {
			return start_retained_admin_merge(
				runtime,
				lane,
				RetainedAdminMergeReasons {
					admin_merge_unavailable: "external_review_admin_merge_unavailable",
					admin_merge_failed: "external_review_admin_merge_failed",
				},
			);
		}
		if review_state_landing_requires_agent_fallback(&lane.review_state) {
			return write_retained_review_orchestration_marker(
				runtime.state_store,
				lane,
				ReviewOrchestrationPhase::RepairRequired,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		}
		if phase == ReviewOrchestrationPhase::WaitingForResult {
			return write_retained_review_orchestration_marker(
				runtime.state_store,
				lane,
				ReviewOrchestrationPhase::PassWaitingForGates,
				RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker),
			);
		}

		return Ok(());
	}
	if external_review_result_arrived(&lane.review_state, &lane.orchestration_marker) {
		return apply_passive_retained_manual_attention(
			passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			&lane.orchestration_marker,
			"external_review_pass_signal_missing",
		);
	}

	Ok(())
}

fn external_review_requires_repair(
	review_state: &PullRequestReviewState,
	marker: &ReviewOrchestrationMarker,
) -> bool {
	review_state.unresolved_review_threads > 0
		|| matches!(review_state.review_decision.as_deref(), Some("CHANGES_REQUESTED"))
		|| external_review_has_actionable_feedback(review_state, marker)
}

fn handle_waiting_for_merge_phase<T>(
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
		lane.orchestration_marker.auto_merge_enabled_at_unix_epoch()
	else {
		return Ok(());
	};

	if now_unix_epoch - auto_merge_enabled_at_unix_epoch
		<= EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS
	{
		return Ok(());
	}

	apply_passive_retained_manual_attention(
		PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		&lane.orchestration_marker,
		timeout_reason,
	)
}
