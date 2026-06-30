pub(crate) struct RetainedReviewLane {
	snapshot: PostReviewLaneSnapshot,
	review_state: PullRequestReviewState,
	orchestration_marker: ReviewOrchestrationMarker,
}

struct RetainedReviewRuntime<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	github_token: &'a mut Option<String>,
	now_unix_epoch: i64,
}

struct PassiveRetainedAttentionRuntime<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
}
impl<T> Clone for PassiveRetainedAttentionRuntime<'_, T> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T> Copy for PassiveRetainedAttentionRuntime<'_, T> {}

#[derive(Clone, Copy)]
struct RetainedReviewOrchestrationMarkerFields {
	request_comment_database_id: Option<i64>,
	request_created_at_unix_epoch: Option<i64>,
	request_retry_count: i64,
	external_round_count: i64,
	auto_merge_enabled_at_unix_epoch: Option<i64>,
}
impl RetainedReviewOrchestrationMarkerFields {
	fn from_marker(marker: &ReviewOrchestrationMarker) -> Self {
		Self {
			request_comment_database_id: marker.request_comment_database_id(),
			request_created_at_unix_epoch: marker.request_created_at_unix_epoch(),
			request_retry_count: marker.request_retry_count(),
			external_round_count: marker.external_round_count(),
			auto_merge_enabled_at_unix_epoch: marker.auto_merge_enabled_at_unix_epoch(),
		}
	}
}

#[derive(Clone, Copy)]
struct RetainedAdminMergeReasons {
	admin_merge_unavailable: &'static str,
	admin_merge_failed: &'static str,
}

enum RetainedReviewLaneReviewLoad {
	Skip,
	Blocked(String),
	ReviewState(Box<PullRequestReviewState>),
}

pub(crate) fn worktree_mapping_is_stale_terminal_local_residue(
	project: &ServiceConfig,
	state_store: &StateStore,
	mapping: &WorktreeMapping,
	active_issue_ids: &HashSet<String>,
) -> Result<bool> {
	if active_issue_ids.contains(mapping.issue_id())
		|| !looks_like_tracker_issue_identifier_key(mapping.issue_id())
		|| mapping.provenance().source() != WORKTREE_PROVENANCE_RUNTIME_RECORDED
	{
		return Ok(false);
	}
	if state_store.issue_has_active_shared_claim(project.service_id(), mapping.issue_id())? {
		return Ok(false);
	}
	if state_store.issue_has_review_lifecycle_record(project.service_id(), mapping.issue_id())?
		|| state_store
			.issue_has_review_policy_checkpoint(project.service_id(), mapping.issue_id())?
	{
		return Ok(false);
	}
	if mapping.worktree_path().try_exists()? {
		return Ok(false);
	}

	let Some(attempt) = state_store.latest_run_attempt_for_issue(mapping.issue_id())? else {
		return Ok(false);
	};

	Ok(local_run_attempt_status_is_terminal(attempt.status()))
}

fn reconcile_post_review_orchestration<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<()>
where
	T: IssueTracker,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	reconcile_post_review_orchestration_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		&review_state_inspector,
	)
}

fn reconcile_post_review_orchestration_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<()>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let active_issue_ids = state_store
		.list_active_shared_leases(project.service_id())?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<HashSet<_>>();
	let worktrees = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.filter_map(|mapping| {
			match worktree_mapping_is_stale_terminal_local_residue(
				project,
				state_store,
				&mapping,
				&active_issue_ids,
			) {
				Ok(true) => None,
				Ok(false) => Some(Ok(mapping)),
				Err(error) => Some(Err(error)),
			}
		})
		.collect::<Result<Vec<_>>>()?;

	if worktrees.is_empty() {
		return Ok(());
	}

	let issue_ids =
		worktrees.iter().map(|mapping| mapping.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = tracker.refresh_issues(&issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let opt_out_label = tracker_policy.opt_out_label();
	let needs_attention_label = tracker_policy.needs_attention_label();
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut github_token: Option<String> = None;

	for worktree in worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id()).cloned() else {
			continue;
		};

		if !eligible_post_review_orchestration_issue(
			tracker,
			&issue,
			project.service_id(),
			success_state,
			opt_out_label,
			needs_attention_label,
		)? {
			continue;
		}
		if state_store.lease_for_issue(&issue.id)?.is_some() {
			continue;
		}

		let lane = match load_retained_review_lane(
			project.service_id(),
			state_store,
			issue,
			worktree,
			review_state_inspector,
		)? {
			RetainedReviewLaneLoad::Skip => continue,
			RetainedReviewLaneLoad::Wait(reason) => {
				tracing::info!(
					project_id = project.service_id(),
					reason = reason.as_str(),
					"Retained post-review orchestration is waiting for transient readback recovery."
				);

				continue;
			},
			RetainedReviewLaneLoad::Blocked(blocked) => {
				apply_passive_retained_manual_attention_with_run_identity(
					PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
					&blocked.issue,
					&blocked.worktree,
					&blocked.run_identity,
					&blocked.reason,
				)?;

				continue;
			},
			RetainedReviewLaneLoad::Ready(lane) => *lane,
		};

		if let Some(reason) = validate_review_orchestration_marker(
			&lane.snapshot,
			&lane.review_state,
			&lane.orchestration_marker,
		) {
			apply_passive_retained_manual_attention(
				PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
				&lane.snapshot.issue,
				&lane.snapshot.worktree,
				&lane.orchestration_marker,
				reason,
			)?;

			continue;
		}

		reconcile_retained_review_lane(
			tracker,
			project,
			workflow,
			state_store,
			&lane,
			&mut github_token,
			now_unix_epoch,
		)?;
	}

	Ok(())
}

fn eligible_post_review_orchestration_issue<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	success_state: &str,
	opt_out_label: &str,
	needs_attention_label: &str,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	Ok(tracker::issue_has_label_with_server_confirmation(
		tracker,
		issue,
		&tracker::automation_active_label(service_id),
	)? && issue.state.name == success_state
		&& !issue.has_label(opt_out_label)
		&& !issue.has_label(needs_attention_label))
}

fn load_retained_review_lane<I>(
	project_id: &str,
	state_store: &StateStore,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	review_state_inspector: &I,
) -> Result<RetainedReviewLaneLoad>
where
	I: PullRequestReviewStateInspector,
{
	let review_handoff =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?;
	let Some(review_handoff) = review_handoff else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			None,
			"missing_review_handoff_record",
		));
	};
	let local_branch_name = match worktree_checkout_branch_name(worktree.worktree_path()) {
		Ok(local_branch_name) => local_branch_name,
		Err(_error) => {
			return Ok(RetainedReviewLaneLoad::Wait(String::from(
				"worktree_checkout_branch_read_failed",
			)));
		},
	};
	let Some(local_branch_name) = local_branch_name else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			Some(&review_handoff),
			"worktree_checkout_branch_missing",
		));
	};
	let local_head_oid = match worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) => {
			return Ok(RetainedReviewLaneLoad::Wait(String::from("worktree_head_read_failed")));
		},
	};
	let Some(local_head_oid) = local_head_oid else {
		return Ok(blocked_retained_review_lane(
			issue,
			worktree,
			Some(&review_handoff),
			"worktree_head_missing",
		));
	};
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(review_handoff.clone()),
		local_branch_name: Some(local_branch_name),
		local_head_oid: Some(local_head_oid.clone()),
	};
	let review_state =
		match load_retained_review_lane_review_state(&snapshot, review_state_inspector)? {
			RetainedReviewLaneReviewLoad::Skip => return Ok(RetainedReviewLaneLoad::Skip),
			RetainedReviewLaneReviewLoad::Blocked(reason) =>
				return Ok(blocked_retained_review_lane(
					snapshot.issue,
					snapshot.worktree,
					Some(&review_handoff),
					&reason,
				)),
			RetainedReviewLaneReviewLoad::ReviewState(review_state) => *review_state,
		};
	let orchestration_marker = ensure_review_orchestration_marker(
		project_id,
		state_store,
		&snapshot.issue,
		&review_handoff,
		&local_head_oid,
	)?;

	Ok(RetainedReviewLaneLoad::Ready(Box::new(RetainedReviewLane {
		snapshot,
		review_state,
		orchestration_marker,
	})))
}

fn load_retained_review_lane_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> Result<RetainedReviewLaneReviewLoad>
where
	I: PullRequestReviewStateInspector,
{
	let review_state = match load_post_review_lane_review_state(snapshot, review_state_inspector)? {
		PostReviewLaneStateLoad::Classification(classification) =>
			return Ok(retained_review_lane_review_load_from_classification(classification)),
		PostReviewLaneStateLoad::ReviewState(review_state) => Box::new(review_state),
	};

	if review_state.state == "MERGED" {
		return Ok(RetainedReviewLaneReviewLoad::Skip);
	}
	if review_state.state != "OPEN" {
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from("pull_request_not_open")));
	}
	if review_state.is_draft {
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from("pull_request_is_draft")));
	}

	Ok(RetainedReviewLaneReviewLoad::ReviewState(review_state))
}

fn retained_review_lane_review_load_from_classification(
	classification: PostReviewLaneClassification,
) -> RetainedReviewLaneReviewLoad {
	if classification.decision == PostReviewLaneDecision::Block {
		RetainedReviewLaneReviewLoad::Blocked(classification.reason)
	} else {
		RetainedReviewLaneReviewLoad::Skip
	}
}

fn blocked_retained_review_lane(
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	review_handoff: Option<&ReviewHandoffMarker>,
	reason: &str,
) -> RetainedReviewLaneLoad {
	let (run_id, attempt_number) =
		retained_review_run_identity(worktree.worktree_path(), review_handoff);

	RetainedReviewLaneLoad::Blocked(Box::new(RetainedReviewLaneBlocked {
		issue,
		worktree,
		run_identity: RetainedReviewRunIdentity { run_id, attempt_number },
		reason: reason.to_owned(),
	}))
}

fn retained_review_run_identity(
	worktree_path: &Path,
	review_handoff: Option<&ReviewHandoffMarker>,
) -> (String, i64) {
	if let Some(review_handoff) = review_handoff {
		return (review_handoff.run_id().to_owned(), review_handoff.attempt_number());
	}
	if let Ok(Some(marker)) = state::read_run_activity_marker_snapshot(worktree_path) {
		return (marker.run_id().to_owned(), marker.attempt_number());
	}

	(String::from("retained-review-orchestration"), 1)
}

fn reconcile_retained_review_lane<T>(
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

fn retained_review_merge_subject(lane: &RetainedReviewLane) -> Result<String> {
	let review_handoff = lane.snapshot.review_handoff.as_ref().ok_or_else(|| {
		eyre::eyre!(
			"Retained admin merge for `{}` requires a matching runtime review handoff on branch `{}`.",
			lane.snapshot.issue.identifier,
			lane.snapshot.worktree.branch_name(),
		)
	})?;

	if review_handoff.pr_head_oid() != lane.orchestration_marker.head_sha() {
		eyre::bail!(
			"Retained admin merge for `{}` requires review handoff head `{}` to match orchestration head `{}`.",
			lane.snapshot.issue.identifier,
			review_handoff.pr_head_oid(),
			lane.orchestration_marker.head_sha(),
		);
	}

	let head_subject = retained_review_head_commit_subject(
		lane.snapshot.worktree.worktree_path(),
		lane.orchestration_marker.head_sha(),
	)?;

	commit_message::build_landed_merge_commit_message(
		&head_subject,
		&lane.snapshot.issue.identifier,
	)
}

fn retained_review_head_commit_subject(worktree_path: &Path, head_sha: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["log", "-1", "--format=%s"])
		.arg(head_sha)
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to read retained review head commit subject `{}` in `{}`: {}",
			head_sha,
			worktree_path.display(),
			stderr.trim()
		);
	}

	String::from_utf8(output.stdout)
		.map(|stdout| stdout.trim_end_matches(['\n', '\r']).to_owned())
		.map_err(Into::into)
}

fn start_retained_admin_merge<T>(
	runtime: &mut RetainedReviewRuntime<'_, T>,
	lane: &RetainedReviewLane,
	reasons: RetainedAdminMergeReasons,
) -> Result<()>
where
	T: IssueTracker,
{
	if let Some(reason) = authority_boundary_landing_requirement(
		&lane.snapshot,
		Some(PostReviewRuntimeState {
			state_store: runtime.state_store,
			project_id: runtime.project.service_id(),
			review_level: runtime.project.codex().review_level(),
		}),
	)? {
		tracing::info!(
			project_id = runtime.project.service_id(),
			issue_id = lane.snapshot.issue.id,
			issue = lane.snapshot.issue.identifier,
			reason,
			"Retained admin merge is waiting for authority-boundary landing clearance."
		);

		return Ok(());
	}

	if !lane.review_state.merge_commit_allowed {
		return apply_passive_retained_manual_attention(
			passive_attention_runtime(runtime),
			&lane.snapshot.issue,
			&lane.snapshot.worktree,
			&lane.orchestration_marker,
			reasons.admin_merge_unavailable,
		);
	}

	let merge_subject = match retained_review_merge_subject(lane) {
		Ok(subject) => subject,
		Err(error) => {
			tracing::warn!(
				issue_id = lane.snapshot.issue.id,
				issue = lane.snapshot.issue.identifier,
				branch = lane.snapshot.worktree.branch_name(),
				?error,
				"Retained admin merge could not derive a compliant landed change record."
			);

			return apply_passive_retained_manual_attention(
				passive_attention_runtime(runtime),
				&lane.snapshot.issue,
				&lane.snapshot.worktree,
				&lane.orchestration_marker,
				"retained_admin_merge_subject_unavailable",
			);
		},
	};
	let github_token = retained_review_github_token(runtime.project, &mut *runtime.github_token)?;
	let merge_succeeded = match github::admin_merge_pull_request(
		lane.snapshot.worktree.worktree_path(),
		lane.review_state.url.as_str(),
		lane.orchestration_marker.head_sha(),
		Some(merge_subject.as_str()),
		github_token,
		runtime.project.github().command_path(),
	) {
		Ok(()) => true,
		Err(_error) => matches!(
			github::pull_request_is_merged_at_head(
				lane.snapshot.worktree.worktree_path(),
				lane.review_state.url.as_str(),
				lane.orchestration_marker.head_sha(),
				github_token,
				runtime.project.github().command_path(),
			),
			Ok(true)
		),
	};

	if merge_succeeded {
		return write_retained_review_orchestration_marker(
			runtime.state_store,
			lane,
			ReviewOrchestrationPhase::WaitingForMerge,
			RetainedReviewOrchestrationMarkerFields {
				auto_merge_enabled_at_unix_epoch: Some(runtime.now_unix_epoch),
				..RetainedReviewOrchestrationMarkerFields::from_marker(&lane.orchestration_marker)
			},
		);
	}

	apply_passive_retained_manual_attention(
		passive_attention_runtime(runtime),
		&lane.snapshot.issue,
		&lane.snapshot.worktree,
		&lane.orchestration_marker,
		reasons.admin_merge_failed,
	)
}

fn retained_review_github_token<'a>(
	project: &ServiceConfig,
	github_token: &'a mut Option<String>,
) -> Result<&'a str> {
	if github_token.is_none() {
		*github_token = Some(resolve_configured_env_var(
			"github.token_env_var",
			Some(project.github().token_env_var()),
		)?);
	}

	github_token.as_deref().ok_or_else(|| {
		eyre::eyre!("Retained review orchestration requires a configured GitHub token.")
	})
}

fn write_retained_review_orchestration_marker(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	phase: ReviewOrchestrationPhase,
	fields: RetainedReviewOrchestrationMarkerFields,
) -> Result<()> {
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

fn ensure_review_orchestration_marker(
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

	state_store.upsert_review_orchestration_marker(project_id, &issue.id, &marker)?;

	Ok(marker)
}

fn apply_passive_retained_manual_attention<T>(
	runtime: PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	orchestration_marker: &ReviewOrchestrationMarker,
	reason: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	apply_passive_retained_manual_attention_with_run_identity(
		runtime,
		issue,
		worktree,
		&RetainedReviewRunIdentity {
			run_id: orchestration_marker.run_id().to_owned(),
			attempt_number: orchestration_marker.attempt_number(),
		},
		reason,
	)
}

fn apply_passive_retained_manual_attention_with_run_identity<T>(
	runtime: PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	run_identity: &RetainedReviewRunIdentity,
	reason: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	if passive_retained_attention_blocker_was_resolved(&runtime, issue, worktree, reason)? {
		return Ok(());
	}

	let synthetic_issue_run = IssueRunPlan {
		issue: issue.clone(),
		issue_state: issue.state.name.clone(),
		initial_issue_state: issue.state.name.clone(),
		worktree: WorktreeSpec {
			branch_name: worktree.branch_name().to_owned(),
			issue_identifier: issue.identifier.clone(),
			path: worktree.worktree_path().to_path_buf(),
			reused_existing: true,
		},
		#[cfg(test)]
		retry_project_slug: String::new(),
		dispatch_mode: IssueDispatchMode::ReviewRepair,
		attempt_number: run_identity.attempt_number,
		run_id: run_identity.run_id.clone(),
		retry_budget_base: 0,
	};
	let worktree_path = relative_worktree_path_for_path(
		runtime.project,
		synthetic_issue_run.worktree.path.as_path(),
	);
	let privacy_classifier = configured_public_projection_privacy_classifier(runtime.project)?;
	let _ = apply_terminal_failure_writeback(
		runtime.tracker,
		TerminalFailureWritebackRuntime {
			service_id: runtime.project.service_id(),
			state_store: Some(runtime.state_store),
			privacy_classifier: &privacy_classifier,
		},
		runtime.workflow,
		&synthetic_issue_run,
		&worktree_path,
		true,
		&Report::new(RetainedReviewNeedsAttention { reason: reason.to_owned() }),
	)?;

	Ok(())
}

fn passive_retained_attention_blocker_was_resolved<T>(
	runtime: &PassiveRetainedAttentionRuntime<'_, T>,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	reason: &str,
) -> Result<bool>
where
	T: IssueTracker,
{
	if reason != "missing_review_handoff_record" {
		return Ok(false);
	}

	let Some(review_handoff) = runtime.state_store.review_handoff_marker(
		runtime.project.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?
	else {
		return Ok(false);
	};

	tracing::info!(
		service_id = runtime.project.service_id(),
		issue_id = issue.id.as_str(),
		issue = issue.identifier.as_str(),
		branch = worktree.branch_name(),
		pr_url = review_handoff.pr_url(),
		pr_head_sha = review_handoff.pr_head_oid(),
		"Skipping stale retained review attention writeback because review handoff is now rebound."
	);

	Ok(true)
}

fn passive_attention_runtime<'a, T>(
	runtime: &'a RetainedReviewRuntime<'_, T>,
) -> PassiveRetainedAttentionRuntime<'a, T> {
	PassiveRetainedAttentionRuntime {
		tracker: runtime.tracker,
		project: runtime.project,
		workflow: runtime.workflow,
		state_store: runtime.state_store,
	}
}
