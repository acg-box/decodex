use state::IssueLease;
use state::PreacquiredLeaseGuards;
use state::WORKTREE_PROVENANCE_RUNTIME_RECORDED;

use crate::commit_message;

const INTERNAL_RETAINED_DRAIN_MAX_PASSES: usize = 2;

struct RetainedReviewLane {
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

struct ProjectStateReconciliationContext<'a, T> {
	tracker: &'a T,
	project: &'a ServiceConfig,
	workflow: &'a WorkflowDocument,
	state_store: &'a StateStore,
	worktree_manager: &'a WorktreeManager,
}

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
		|| state_store.issue_has_review_policy_checkpoint(project.service_id(), mapping.issue_id())?
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

fn run_configured_cycle(
	request: RunCycleRequest<'_>,
) -> Result<Option<RunSummary>> {
	let config = ServiceConfig::from_path(request.config_path)?;
	let workflow = load_configured_cycle_workflow(&config, request.preferred_workflow_snapshot)?;
	let api_key = config.tracker().resolve_api_key()?;
	let tracker = LinearClient::new(api_key)?;

	if let Some(issue_id) = request.preferred_issue_id {
		let target_context = TargetIssueRunContext {
			tracker: &tracker,
			project: &config,
			workflow: &workflow,
			state_store: request.state_store,
			issue_id,
			preferred_issue_state: request.preferred_issue_state,
			preferred_initial_issue_state: request.preferred_initial_issue_state,
			dry_run: request.dry_run,
			lease_preacquired: request.preferred_lease_acquired,
			preferred_issue_claim_fd: request.preferred_issue_claim_fd,
			preferred_dispatch_slot_fd: request.preferred_dispatch_slot_fd,
			preferred_dispatch_slot_index: request.preferred_dispatch_slot_index,
			dispatch_mode: request.preferred_dispatch_mode.unwrap_or(IssueDispatchMode::Normal),
			preferred_run_identity: request.preferred_run_identity,
			preferred_retry_budget_base: request.preferred_retry_budget_base,
		};

		return match request.preferred_dispatch_mode {
			Some(_) => run_target_issue_once(target_context),
			None => run_target_issue_once_with_inferred_dispatch(target_context),
		};
	}

	run_project_once(&tracker, &config, &workflow, request.state_store, request.dry_run)
}

fn load_configured_cycle_workflow(
	config: &ServiceConfig,
	preferred_workflow_snapshot: Option<&str>,
) -> Result<WorkflowDocument> {
	let workflow_path = config.workflow_path().to_path_buf();

	match preferred_workflow_snapshot {
		Some(snapshot) => WorkflowDocument::parse_markdown(snapshot),
		None => WorkflowDocument::from_path(&workflow_path),
	}
}

fn run_project_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_project_once_with_exclusions(tracker, project, workflow, state_store, dry_run, &[])
}

fn run_project_once_with_exclusions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let Some(issue_run) = plan_project_issue_run_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		excluded_issue_ids,
	)?
	else {
		if !dry_run {
			reconcile_terminal_thread_archive_backlog_best_effort(project, workflow, state_store);
		}

		return Ok(None);
	};

	complete_issue_run(tracker, project, workflow, state_store, issue_run, dry_run)
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
			return Ok(RetainedReviewLaneLoad::Wait(String::from(
				"worktree_head_read_failed",
			)));
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
	let review_state = match load_retained_review_lane_review_state(
		&snapshot,
		review_state_inspector,
	)? {
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
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from(
			"pull_request_not_open",
		)));
	}
	if review_state.is_draft {
		return Ok(RetainedReviewLaneReviewLoad::Blocked(String::from(
			"pull_request_is_draft",
		)));
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

	let phase = ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase())
		.map_err(|error| eyre::eyre!("Failed to parse retained review orchestration phase: {error}"))?;

	match phase {
		ReviewOrchestrationPhase::RequestPending => handle_request_pending_phase(
			project,
			state_store,
			lane,
			github_token,
		),
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
	let phase = ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase())
		.map_err(|error| eyre::eyre!("Failed to parse retained review orchestration phase: {error}"))?;

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
		)
		|| merge_state_requires_review_repair(
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
) -> Result<()>
{
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
		Err(_error) =>
			matches!(
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

	github_token
		.as_deref()
		.ok_or_else(|| eyre::eyre!("Retained review orchestration requires a configured GitHub token."))
}

fn write_retained_review_orchestration_marker(
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	phase: ReviewOrchestrationPhase,
	fields: RetainedReviewOrchestrationMarkerFields,
) -> Result<()>
{
	let local_head_oid = lane
		.snapshot
		.local_head_oid
		.as_deref()
		.ok_or_else(|| eyre::eyre!("Retained review orchestration requires a local lane HEAD."))?;
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
) -> Result<ReviewOrchestrationMarker>
{
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

			state_store.upsert_review_orchestration_marker(project_id, &issue.id, &rebound_marker)?;

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
	let worktree_path =
		relative_worktree_path_for_path(runtime.project, synthetic_issue_run.worktree.path.as_path());
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
	)? else {
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

fn plan_project_issue_run_with_exclusions<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());

	state_store.configure_dispatch_slot_root(
		project.service_id(),
		project.worktree_root(),
		workflow.frontmatter().execution().max_concurrent_agents(),
	)?;

	let recovered_state =
		recover_runtime_state_from_tracker_and_worktrees(tracker, project, workflow, state_store)?;

	if !dry_run {
		reconcile_project_state(tracker, project, workflow, state_store, &worktree_manager)?;
		reconcile_post_review_orchestration(tracker, project, workflow, state_store)?;
	}

	let Some(selected_issue) = select_project_issue_run_candidate(
		tracker,
		project,
		workflow,
		state_store,
		recovered_state,
		dry_run,
		excluded_issue_ids,
	)? else {
		return Ok(None);
	};
	let mut refreshed_issues = tracker.refresh_issues(slice::from_ref(&selected_issue.issue.id))?;
	let Some(issue) = refreshed_issues.pop() else {
		return Ok(None);
	};
	let dispatch_mode = selected_issue.dispatch_mode;
	let preferred_run_identity = selected_issue.preferred_run_identity;
	let concurrency = ConcurrencySnapshot::new(project.service_id(), state_store)?;

	if !dry_run && dispatch_mode != IssueDispatchMode::Closeout {
		ensure_project_has_no_merged_worktree_cleanup_debt(project)?;
	}
	if !concurrency.has_global_capacity(workflow.frontmatter().execution()) {
		return Ok(None);
	}
	if !dispatch_mode.allows_issue(
		tracker,
		&issue,
		project,
		workflow,
		state_store,
		RetryIssueStateHint::default(),
	)? {
		if dispatch_mode == IssueDispatchMode::Closeout
			&& let Some(reason) =
					closeout_dispatch_block_reason(tracker, &issue, project, workflow, state_store)?
		{
			if !dry_run {
				eyre::bail!("retained closeout dispatch blocked: {reason}");
			}

			return Ok(None);
		}

			return replan_project_issue_run_after_excluding(
				tracker,
				project,
				workflow,
				state_store,
				dry_run,
				excluded_issue_ids,
				issue.id.as_str(),
			);
	}

	let Some(issue_run) = prepare_issue_run(
		PrepareIssueRunContext {
			tracker,
			project,
			workflow,
			state_store,
			worktree_manager: &worktree_manager,
			dry_run,
			lease_preacquired: false,
			dispatch_mode,
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_run_identity: preferred_run_identity.as_ref().map(|identity| {
				PreferredRunIdentity {
					run_id: identity.run_id.as_str(),
					attempt_number: identity.attempt_number,
				}
				}),
				preferred_retry_budget_base: None,
			},
		issue,
	)?
	else {
		return Ok(None);
	};

	Ok(Some(issue_run))
}

fn select_project_issue_run_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	dry_run: bool,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let selected_retry_issue =
		select_recovered_retry_issue_candidate(project, state_store, recovered_state, excluded_issue_ids)?;
	let selected_post_review_issue = select_post_review_issue_candidate(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
	)?;

	if let Some(candidate) = selected_retry_issue.or(selected_post_review_issue) {
		return Ok(Some(candidate));
	}
	if let Some(candidate) =
		select_execution_program_run_candidate(tracker, project, workflow, state_store, excluded_issue_ids)?
	{
		return Ok(Some(candidate));
	}

	let issues = queued_issues_for_dispatch(tracker, project, workflow, dry_run)?;

	Ok(select_issue_candidate_with_exclusions(
		tracker,
		issues,
		workflow,
		state_store,
		project.service_id(),
		excluded_issue_ids,
	)?
	.map(|issue| SelectedIssueRunCandidate::new(issue, IssueDispatchMode::Normal)))
}

fn select_recovered_retry_issue_candidate(
	project: &ServiceConfig,
	state_store: &StateStore,
	recovered_state: RecoveredRuntimeState,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>> {
	for issue in recovered_state.recoverable_issues {
		if excluded_issue_ids.contains(&issue.id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
			continue;
		}

		return Ok(Some(SelectedIssueRunCandidate::new(
			issue,
			IssueDispatchMode::Retry,
		)));
	}

	Ok(None)
}

fn queued_issues_for_dispatch<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let queue_label = tracker::automation_queue_label(project.service_id());

	clear_terminal_queued_lane_labels(
		tracker,
		project,
		workflow,
		tracker.list_issues_with_label(&queue_label)?,
		dry_run,
	)
}

fn clear_terminal_queued_lane_labels<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	issues: Vec<TrackerIssue>,
	dry_run: bool,
) -> Result<Vec<TrackerIssue>>
where
	T: IssueTracker,
{
	let mut nonterminal_issues = Vec::with_capacity(issues.len());

	for issue in issues {
		if is_terminal_issue(&issue, workflow) {
			if !dry_run {
				tracker::clear_automation_lane_labels(tracker, &issue, project.service_id())?;

				tracing::info!(
					project_id = project.service_id(),
					issue_id = issue.id,
					issue = issue.identifier,
					"Cleared automation lane labels from terminal queued issue."
				);
			}

			continue;
		}

		nonterminal_issues.push(issue);
	}

	Ok(nonterminal_issues)
}

fn replan_project_issue_run_after_excluding<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	dry_run: bool,
	excluded_issue_ids: &[&str],
	issue_id: &str,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let mut next_excluded_issue_ids = excluded_issue_ids.to_vec();

	next_excluded_issue_ids.push(issue_id);

	plan_project_issue_run_with_exclusions(
		tracker,
		project,
		workflow,
		state_store,
		dry_run,
		&next_excluded_issue_ids,
	)
}

fn select_post_review_issue_candidate<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
{
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};

	select_post_review_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		&review_state_inspector,
	)
}

fn select_post_review_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	if let Some(issue) = select_post_review_repair_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		review_state_inspector,
	)? {
		return Ok(Some(SelectedIssueRunCandidate::new(
			issue,
			IssueDispatchMode::ReviewRepair,
		)));
	}

	select_post_review_closeout_issue_candidate_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		excluded_issue_ids,
		review_state_inspector,
	)
}

fn select_post_review_repair_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| lane.classification == "needs_review_repair")
		.filter(|lane| !excluded_issue_ids.contains(&lane.issue_id.as_str()))
		.map(|lane| lane.issue_id.clone())
		.collect::<Vec<_>>();

	if candidate_issue_ids.is_empty() {
		return Ok(None);
	}

	let issues = tracker.refresh_issues(&candidate_issue_ids)?;
	let mut issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	for lane in lanes {
		if lane.classification != "needs_review_repair" {
			continue;
		}
		if excluded_issue_ids.contains(&lane.issue_id.as_str()) {
			continue;
		}
		if state_store.issue_has_active_shared_claim(project.service_id(), &lane.issue_id)? {
			continue;
		}

		if let Some(issue) = issues_by_id.remove(&lane.issue_id) {
			return Ok(Some(issue));
		}
	}

	Ok(None)
}

fn select_post_review_closeout_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	excluded_issue_ids: &[&str],
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let candidate_issue_ids = lanes
		.iter()
		.filter(|lane| post_review_lane_is_closeout_candidate(lane, completed_state))
		.filter(|lane| !excluded_issue_ids.contains(&lane.issue_id.as_str()))
		.map(|lane| lane.issue_id.clone())
		.collect::<Vec<_>>();

	if candidate_issue_ids.is_empty() {
		return Ok(None);
	}

	let issues = tracker.refresh_issues(&candidate_issue_ids)?;
	let mut issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	for lane in lanes {
		let is_closeout_candidate = post_review_lane_is_closeout_candidate(&lane, completed_state);

		if !is_closeout_candidate {
			continue;
		}
		if excluded_issue_ids.contains(&lane.issue_id.as_str()) {
			continue;
		}

		if let Some(issue) = issues_by_id.remove(&lane.issue_id) {
			if closeout_lane_active_claim_blocks_dispatch(project, state_store, &issue)? {
				continue;
			}

			let preferred_run_identity =
				retained_closeout_preferred_run_identity(state_store, project.service_id(), &issue)?;

			return Ok(Some(SelectedIssueRunCandidate {
				issue,
				dispatch_mode: IssueDispatchMode::Closeout,
				preferred_run_identity,
			}));
		}
	}

	Ok(None)
}

fn retained_closeout_preferred_run_identity(
	state_store: &StateStore,
	project_id: &str,
	issue: &TrackerIssue,
) -> Result<Option<RetainedReviewRunIdentity>>
{
	let Some(worktree) = state_store.worktree_for_issue(&issue.id)? else {
		return Ok(None);
	};
	let Some(review_handoff) =
		state_store.review_handoff_marker(project_id, &issue.id, worktree.branch_name())?
	else {
		return Ok(None);
	};
	let identity = RetainedReviewRunIdentity {
		run_id: review_handoff.run_id().to_owned(),
		attempt_number: review_handoff.attempt_number(),
	};

	if retained_closeout_run_identity_is_reusable(state_store, &issue.id, &identity)?
		|| retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
			state_store,
			&issue.id,
			&identity,
			&worktree,
		)?
	{
		return Ok(Some(identity));
	}

	Ok(None)
}

fn retained_closeout_run_identity_is_reusable(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(true);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}

	Ok(!matches!(
		existing_attempt.status(),
		"failed" | "interrupted" | TERMINAL_GUARDED_RUN_STATUS
	))
}

fn retained_closeout_handoff_identity_is_reusable_after_parent_reconciliation(
	state_store: &StateStore,
	issue_id: &str,
	identity: &RetainedReviewRunIdentity,
	worktree: &WorktreeMapping,
) -> Result<bool> {
	if state_store.issue_has_retry_budget_attempt_after(issue_id, identity.attempt_number)? {
		return Ok(false);
	}

	let Some(existing_attempt) = state_store.run_attempt(&identity.run_id)? else {
		return Ok(false);
	};

	if existing_attempt.issue_id() != issue_id
		|| existing_attempt.attempt_number() != identity.attempt_number
	{
		return Ok(false);
	}
	if !matches!(existing_attempt.status(), "failed" | "interrupted") {
		return Ok(false);
	}
	if worktree_has_retry_schedule_for_run(worktree.worktree_path(), identity)? {
		return Ok(false);
	}

	Ok(true)
}

fn worktree_has_retry_schedule_for_run(
	worktree_path: &Path,
	identity: &RetainedReviewRunIdentity,
) -> Result<bool> {
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == identity.run_id
		&& marker.attempt_number() == identity.attempt_number
		&& marker.retry_kind().is_some())
}

fn post_review_lane_is_closeout_candidate(
	lane: &OperatorPostReviewLaneStatus,
	_completed_state: &str,
) -> bool {
	lane.classification == "continue" && lane.reason == "pull_request_merged_closeout_pending"
}

fn post_review_lane_is_repair_candidate(lane: &OperatorPostReviewLaneStatus) -> bool {
	lane.classification == "needs_review_repair"
}

fn run_target_issue_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let worktree_manager = WorktreeManager::new(
		context.project.service_id(),
		context.project.repo_root(),
		context.project.worktree_root(),
	);

	context.state_store.configure_dispatch_slot_root(
		context.project.service_id(),
		context.project.worktree_root(),
		context.workflow.frontmatter().execution().max_concurrent_agents(),
	)?;

	let issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;

	if !context.dry_run {
		context.state_store.canonicalize_issue_identity(context.issue_id, &issue_id)?;
	}
	if context.lease_preacquired && !context.dry_run {
		adopt_preacquired_target_issue_lease(&context, &issue_id)?;
	}
	if !context.lease_preacquired {
		recover_runtime_state_from_tracker_and_worktrees(
			context.tracker,
			context.project,
			context.workflow,
			context.state_store,
		)?;

		if !context.dry_run {
			reconcile_project_state(
				context.tracker,
				context.project,
				context.workflow,
				context.state_store,
				&worktree_manager,
			)?;
		}
	}

	let Some(issue) = refresh_issue(context.tracker, &issue_id)? else {
		return Ok(None);
	};
	let closeout_preferred_run_identity =
		target_closeout_preferred_run_identity(&context, &issue)?;
	let preferred_run_identity = preferred_run_identity_with_closeout_fallback(
		context.preferred_run_identity,
		closeout_preferred_run_identity.as_ref(),
	);
	let retry_state_hint = RetryIssueStateHint {
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
	};

	if !context.dispatch_mode.allows_issue(
		context.tracker,
		&issue,
		context.project,
		context.workflow,
		context.state_store,
		retry_state_hint,
	)? {
		ensure_target_closeout_dispatch_is_unblocked(&context, &issue)?;

		return Ok(None);
	}

	let reuses_existing_closeout_claim =
		target_issue_reuses_existing_closeout_claim(&context, &issue_id, &issue)?;

	if target_issue_active_claim_blocks_dispatch(&context, &issue_id, &issue)? {
		return Ok(None);
	}
	if !context.lease_preacquired && !reuses_existing_closeout_claim {
		let concurrency = ConcurrencySnapshot::new(context.project.service_id(), context.state_store)?;

		if !concurrency.has_global_capacity(context.workflow.frontmatter().execution()) {
			return Ok(None);
		}
	}
	if !context.dry_run && context.dispatch_mode != IssueDispatchMode::Closeout {
		ensure_project_has_no_merged_worktree_cleanup_debt(context.project)?;
	}

	let Some(issue_run) = prepare_issue_run(
		PrepareIssueRunContext {
			tracker: context.tracker,
			project: context.project,
			workflow: context.workflow,
			state_store: context.state_store,
			worktree_manager: &worktree_manager,
			dry_run: context.dry_run,
			lease_preacquired: context.lease_preacquired || reuses_existing_closeout_claim,
			dispatch_mode: context.dispatch_mode,
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
				preferred_run_identity,
				preferred_retry_budget_base: context.preferred_retry_budget_base,
			},
		issue,
	)?
	else {
		return Ok(None);
	};

	complete_issue_run(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		issue_run,
		context.dry_run,
	)
}

fn ensure_target_closeout_dispatch_is_unblocked<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue: &TrackerIssue,
) -> Result<()>
where
	T: IssueTracker,
{
	if context.dry_run || context.dispatch_mode != IssueDispatchMode::Closeout {
		return Ok(());
	}

	let Some(reason) = closeout_dispatch_block_reason(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
	)?
	else {
		return Ok(());
	};

	eyre::bail!("retained closeout dispatch blocked: {reason}");
}

fn target_closeout_preferred_run_identity<T>(
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

	retained_closeout_preferred_run_identity(
		context.state_store,
		context.project.service_id(),
		issue,
	)
}

fn preferred_run_identity_with_closeout_fallback<'a>(
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

fn run_target_issue_once_with_inferred_dispatch<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if target_issue_has_status_visible_review_repair(&context)? {
		return run_target_status_visible_review_repair_once(context);
	}
	if target_issue_has_status_visible_closeout(&context)? {
		return run_target_status_visible_closeout_once(context);
	}

	if let Some(summary) = run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::Normal,
	))? {
		return Ok(Some(summary));
	}
	if let Some(summary) = run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::Retry,
	))? {
		return Ok(Some(summary));
	}
	if let Some(summary) = run_target_status_visible_review_repair_once(
		target_issue_run_context_with_dispatch_mode(&context, IssueDispatchMode::ReviewRepair),
	)? {
		return Ok(Some(summary));
	}

	run_target_status_visible_closeout_once(context)
}

fn target_issue_has_status_visible_review_repair<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
	.any(|lane| lane.issue_id == target_issue_id && post_review_lane_is_repair_candidate(&lane)))
}

fn run_target_status_visible_review_repair_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(_issue) = select_target_post_review_repair_issue_candidate_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&target_issue_id,
		context.issue_id,
		&review_state_inspector,
	)? else {
		return Ok(None);
	};

	run_target_issue_once(target_issue_run_context_with_dispatch_mode(
		&context,
		IssueDispatchMode::ReviewRepair,
	))
}

fn target_issue_has_status_visible_closeout<T>(
	context: &TargetIssueRunContext<'_, T>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let completed_state = context.workflow.frontmatter().tracker().resolved_completed_state();
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};

	Ok(build_post_review_lane_statuses(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&review_state_inspector,
	)?
	.into_iter()
		.any(|lane| {
			lane.issue_id == target_issue_id
				&& post_review_lane_is_closeout_candidate(&lane, completed_state)
		}))
}

fn select_target_post_review_repair_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	target_issue_id: &str,
	target_issue_reference: &str,
	review_state_inspector: &I,
) -> Result<Option<TrackerIssue>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let repair_lanes = lanes
		.into_iter()
		.filter(post_review_lane_is_repair_candidate)
		.collect::<Vec<_>>();

	if repair_lanes.is_empty() {
		return Ok(None);
	}

	let Some(target_lane) = repair_lanes
		.iter()
		.find(|lane| lane.issue_id == target_issue_id)
	else {
		let visible_lanes = repair_lanes
			.iter()
			.map(|lane| lane.issue_identifier.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		eyre::bail!(
			"targeted retained review repair mismatch: requested issue `{}` does not match status-visible retained review repair lane(s) `{}`",
			target_issue_reference,
			visible_lanes,
		);
	};
	let issue_ids = [target_lane.issue_id.clone()];
	let mut issues = tracker.refresh_issues(&issue_ids)?;
	let Some(issue_index) = issues.iter().position(|issue| issue.id == target_lane.issue_id) else {
		return Ok(None);
	};
	let issue = issues.swap_remove(issue_index);

	if state_store.issue_has_active_shared_claim(project.service_id(), &issue.id)? {
		return Ok(None);
	}

	Ok(Some(issue))
}

fn run_target_status_visible_closeout_once<T>(
	context: TargetIssueRunContext<'_, T>,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	let target_issue_id = resolve_target_issue_id(context.tracker, context.issue_id)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(context.project.github().token_env_var().to_owned()),
		github_command_path: context.project.github().command_path().map(Path::to_path_buf),
	};
	let Some(candidate) = select_target_post_review_closeout_issue_candidate_with_inspector(
		context.tracker,
		context.project,
		context.workflow,
		context.state_store,
		&target_issue_id,
		context.issue_id,
		&review_state_inspector,
	)? else {
		return Ok(None);
	};
	let preferred_run_identity =
		candidate
			.preferred_run_identity
			.as_ref()
			.map(|identity| PreferredRunIdentity {
				run_id: identity.run_id.as_str(),
				attempt_number: identity.attempt_number,
			});

	run_target_issue_once(TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	})
}

fn select_target_post_review_closeout_issue_candidate_with_inspector<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	target_issue_id: &str,
	target_issue_reference: &str,
	review_state_inspector: &I,
) -> Result<Option<SelectedIssueRunCandidate>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();
	let lanes = build_post_review_lane_statuses(
		tracker,
		project,
		workflow,
		state_store,
		review_state_inspector,
	)?;
	let closeout_lanes = lanes
		.into_iter()
		.filter(|lane| post_review_lane_is_closeout_candidate(lane, completed_state))
		.collect::<Vec<_>>();

	if closeout_lanes.is_empty() {
		return Ok(None);
	}

	let Some(target_lane) = closeout_lanes
		.iter()
		.find(|lane| lane.issue_id == target_issue_id)
	else {
		let visible_lanes = closeout_lanes
			.iter()
			.map(|lane| lane.issue_identifier.as_str())
			.collect::<Vec<_>>()
			.join(", ");

		eyre::bail!(
			"targeted retained closeout mismatch: requested issue `{}` does not match status-visible retained closeout lane(s) `{}`",
			target_issue_reference,
			visible_lanes,
		);
	};
	let issue_ids = [target_lane.issue_id.clone()];
	let mut issues = tracker.refresh_issues(&issue_ids)?;
	let Some(issue_index) = issues.iter().position(|issue| issue.id == target_lane.issue_id) else {
		return Ok(None);
	};
	let issue = issues.swap_remove(issue_index);

	if closeout_lane_active_claim_blocks_dispatch(project, state_store, &issue)? {
		return Ok(None);
	}

	let preferred_run_identity =
		retained_closeout_preferred_run_identity(state_store, project.service_id(), &issue)?;

	Ok(Some(SelectedIssueRunCandidate {
		issue,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity,
	}))
}

fn target_issue_reuses_existing_closeout_claim<T>(
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
	if !context
		.state_store
		.issue_has_active_shared_claim(context.project.service_id(), issue_id)?
	{
		return Ok(false);
	}
	if context.state_store.lease_for_issue(&issue.id)?.is_none() {
		return Ok(false);
	}

	Ok(!closeout_lane_active_claim_blocks_dispatch(
		context.project,
		context.state_store,
		issue,
	)?)
}

fn target_issue_active_claim_blocks_dispatch<T>(
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
	if !context
		.state_store
		.issue_has_active_shared_claim(context.project.service_id(), issue_id)?
	{
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

fn closeout_lane_active_claim_blocks_dispatch(
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

	retained_closeout_lease_has_fresh_activity(&lease, issue, project, now_unix_epoch)
}

fn target_issue_run_context_with_dispatch_mode<'a, T>(
	context: &TargetIssueRunContext<'a, T>,
	dispatch_mode: IssueDispatchMode,
) -> TargetIssueRunContext<'a, T> {
	TargetIssueRunContext {
		tracker: context.tracker,
		project: context.project,
		workflow: context.workflow,
		state_store: context.state_store,
		issue_id: context.issue_id,
		preferred_issue_state: context.preferred_issue_state,
		preferred_initial_issue_state: context.preferred_initial_issue_state,
		dry_run: context.dry_run,
		lease_preacquired: context.lease_preacquired,
		preferred_issue_claim_fd: context.preferred_issue_claim_fd,
		preferred_dispatch_slot_fd: context.preferred_dispatch_slot_fd,
		preferred_dispatch_slot_index: context.preferred_dispatch_slot_index,
		dispatch_mode,
		preferred_run_identity: context.preferred_run_identity,
		preferred_retry_budget_base: context.preferred_retry_budget_base,
	}
}

fn resolve_target_issue_id<T>(tracker: &T, issue_reference: &str) -> Result<String>
where
	T: IssueTracker,
{
	if commit_message::looks_like_issue_identifier(issue_reference)
		&& let Some(issue) = tracker.get_issue_by_identifier(issue_reference)?
	{
		return Ok(issue.id);
	}

	Ok(issue_reference.to_owned())
}

fn adopt_preacquired_target_issue_lease<T>(
	context: &TargetIssueRunContext<'_, T>,
	issue_id: &str,
) -> Result<()>
where
	T: IssueTracker,
{
	let preferred_run_identity = context.preferred_run_identity.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires a planned run identifier")
	})?;
	let preferred_issue_state = context
		.preferred_issue_state
		.ok_or_else(|| eyre::eyre!("daemon child lease handoff requires a planned issue state"))?;
	let issue_claim_fd = context.preferred_issue_claim_fd.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited issue-claim fd")
	})?;
	let dispatch_slot_fd = context.preferred_dispatch_slot_fd.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited dispatch-slot fd")
	})?;
	let dispatch_slot_index = context.preferred_dispatch_slot_index.ok_or_else(|| {
		eyre::eyre!("daemon child lease handoff requires an inherited dispatch-slot index")
	})?;

	context.state_store.adopt_preacquired_lease(
		context.project.service_id(),
		issue_id,
		preferred_run_identity.run_id,
		preferred_issue_state,
		PreacquiredLeaseGuards { issue_claim_fd, dispatch_slot_fd, dispatch_slot_index },
	)?;

	Ok(())
}

fn prepare_issue_run<T>(
	context: PrepareIssueRunContext<'_, T>,
	issue: TrackerIssue,
) -> Result<Option<IssueRunPlan>>
where
	T: IssueTracker,
{
	let planned_worktree = context.worktree_manager.plan_for_issue(&issue.identifier);
	let Some((attempt_number, run_id)) =
		resolve_prepare_run_identity(context.state_store, &issue, context.preferred_run_identity)?
	else {
		return Ok(None);
	};
	let retry_budget_base = retry_budget_base_for_dispatch_mode(
		context.state_store,
		&issue.id,
		&planned_worktree.path,
		context.dispatch_mode,
		context.preferred_retry_budget_base,
	)?;
	let lease_issue_id = issue.id.clone();
	let issue_state = planned_issue_state_for_dispatch(
		context.workflow,
		&issue,
		context.dispatch_mode,
		context.preferred_issue_state,
	);

	if !context.dry_run
		&& !context.lease_preacquired
		&& !context.state_store.try_acquire_lease(
			context.project.service_id(),
			&issue.id,
			&run_id,
			&issue_state,
		)? {
		return Ok(None);
	}

	match (|| -> Result<Option<IssueRunPlan>> {
		let worktree =
			context.worktree_manager.ensure_worktree_with_hooks(
				&issue.identifier,
				context.dry_run,
				context.workflow.frontmatter().execution().workspace_hooks(),
			)?;

		if !context.dry_run {
			context.state_store.upsert_worktree(
				context.project.service_id(),
				&lease_issue_id,
				&worktree.branch_name,
				&worktree.path.display().to_string(),
			)?;
		}

			let Some(refreshed_issue) = refresh_issue(context.tracker, &lease_issue_id)? else {
				return Ok(None);
			};

		if !prepare_issue_run_dispatch_allowed(
			&context,
			&refreshed_issue,
			&lease_issue_id,
			&worktree.branch_name,
			&worktree.path,
		)? {
			return Ok(None);
		}
			if !context.dry_run {
				record_starting_attempt(context.state_store, &run_id, &issue.id, attempt_number)?;
				clear_terminal_guard_marker(&worktree.path)?;
		}

		let initial_issue_state = context
			.preferred_initial_issue_state
			.map_or_else(|| refreshed_issue.state.name.clone(), str::to_owned);
		let issue_run = IssueRunPlan {
			issue: refreshed_issue,
			issue_state: issue_state.clone(),
			initial_issue_state,
			worktree,
			#[cfg(test)]
			retry_project_slug: String::new(),
			dispatch_mode: context.dispatch_mode,
			attempt_number,
			run_id: run_id.clone(),
			retry_budget_base,
		};

		if !context.dry_run {
				write_prepare_lifecycle_events(
					context.tracker,
					context.project,
					context.workflow,
					context.state_store,
					&issue_run,
				)?;
		}

		Ok(Some(issue_run))
	})() {
		Ok(Some(issue_run)) => Ok(Some(issue_run)),
		Ok(None) => {
			clear_prepare_issue_run_lease(
				context.state_store,
				context.dry_run,
				&lease_issue_id,
			)?;

			Ok(None)
		},
		Err(error) => {
			clear_prepare_issue_run_lease(
				context.state_store,
				context.dry_run,
				&lease_issue_id,
			)?;

			Err(error)
		},
	}
}

fn prepare_issue_run_dispatch_allowed<T>(
	context: &PrepareIssueRunContext<'_, T>,
	refreshed_issue: &TrackerIssue,
	lease_issue_id: &str,
	worktree_branch_name: &str,
	worktree_path: &Path,
) -> Result<bool>
where
	T: IssueTracker,
{
	let dispatch_allowed = context.dispatch_mode.allows_issue(
		context.tracker,
		refreshed_issue,
		context.project,
		context.workflow,
		context.state_store,
		RetryIssueStateHint {
			preferred_issue_state: context.preferred_issue_state,
			preferred_initial_issue_state: context.preferred_initial_issue_state,
		},
	)?;

	if !dispatch_allowed {
			if !context.dry_run
				&& context.dispatch_mode == IssueDispatchMode::Closeout
				&& let Some(reason) = closeout_dispatch_block_reason(
					context.tracker,
					refreshed_issue,
					context.project,
					context.workflow,
					context.state_store,
				)?
		{
			eyre::bail!("retained closeout dispatch blocked: {reason}");
		}
		if !context.dry_run && is_terminal_issue(refreshed_issue, context.workflow) {
			cleanup_terminal_worktree(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				lease_issue_id,
				&refreshed_issue.identifier,
				worktree_branch_name,
				worktree_path,
			)?;
		}
	}

	Ok(dispatch_allowed)
}

fn clear_prepare_issue_run_lease(
	state_store: &StateStore,
	dry_run: bool,
	issue_id: &str,
) -> Result<()> {
	if !dry_run {
		state_store.clear_lease(issue_id)?;
	}

	Ok(())
}

fn record_starting_attempt(
	state_store: &StateStore,
	run_id: &str,
	issue_id: &str,
	attempt_number: i64,
) -> Result<()> {
	state_store.record_run_attempt(run_id, issue_id, attempt_number, "starting")
}

fn resolve_prepare_run_identity(
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
		None =>
			Ok(Some((next_attempt_number, build_run_id(&issue.identifier, next_attempt_number)?))),
	}
}

fn complete_issue_run<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	issue_run: IssueRunPlan,
	dry_run: bool,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	if dry_run {
		return Ok(Some(run_summary_from_issue_run(project.service_id(), &issue_run)));
	}

	let summary = execute_issue_run(tracker, project, workflow, state_store, issue_run)?;
	let review_state_inspector = GhPullRequestReviewStateInspector {
		github_token_env_var: Some(project.github().token_env_var().to_owned()),
		github_command_path: project.github().command_path().map(Path::to_path_buf),
	};
	let summary = if let Some(retained_summary) = drain_non_github_review_retained_tail_with_inspector(
		tracker,
		project,
		workflow,
		state_store,
		&summary,
		&review_state_inspector,
			|source_summary| run_retained_closeout_for_handoff_summary(
				tracker,
				project,
				workflow,
				state_store,
				source_summary,
			),
	)? {
		retained_summary
	} else {
		summary
	};

	reconcile_terminal_thread_archive_backlog_best_effort(project, workflow, state_store);

	Ok(Some(summary))
}

fn run_retained_closeout_for_handoff_summary<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	source_summary: &RunSummary,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
{
	run_target_issue_once(TargetIssueRunContext {
		tracker,
		project,
		workflow,
		state_store,
		issue_id: source_summary.issue_id.as_str(),
		preferred_issue_state: None,
		preferred_initial_issue_state: None,
		dry_run: false,
		lease_preacquired: false,
		preferred_issue_claim_fd: None,
		preferred_dispatch_slot_fd: None,
		preferred_dispatch_slot_index: None,
		dispatch_mode: IssueDispatchMode::Closeout,
		preferred_run_identity: None,
		preferred_retry_budget_base: None,
	})
}

fn drain_non_github_review_retained_tail_with_inspector<T, I, F>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	summary: &RunSummary,
	review_state_inspector: &I,
	mut run_closeout: F,
) -> Result<Option<RunSummary>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
	F: FnMut(&RunSummary) -> Result<Option<RunSummary>>,
{
	if project.codex().review_level().uses_github_review()
		|| summary.continuation_pending
		|| !matches!(
			summary.dispatch_mode,
			IssueDispatchMode::Normal | IssueDispatchMode::Program | IssueDispatchMode::ReviewRepair
		)
	{
		return Ok(None);
	}

	let completed_state = workflow.frontmatter().tracker().resolved_completed_state();

	for pass in 0..INTERNAL_RETAINED_DRAIN_MAX_PASSES {
		reconcile_post_review_orchestration_with_inspector(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?;

		let Some(lane) = build_post_review_lane_statuses(
			tracker,
			project,
			workflow,
			state_store,
			review_state_inspector,
		)?
		.into_iter()
		.find(|lane| lane.issue_id == summary.issue_id)
		else {
			return Ok(None);
		};

		if post_review_lane_is_closeout_candidate(&lane, completed_state) {
			if let Some(retained_summary) = run_closeout(summary)? {
				return Ok(Some(retained_summary));
			}

			return Ok(None);
		}
		if lane.reason != "non_github_review_waiting_for_merge"
			|| pass + 1 == INTERNAL_RETAINED_DRAIN_MAX_PASSES
		{
			return Ok(None);
		}
	}

	Ok(None)
}

fn reconcile_project_state<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	worktree_manager: &WorktreeManager,
) -> Result<()>
where
	T: IssueTracker,
{
	let leases = state_store.list_leases(project.service_id())?;
	let mut worktrees = state_store.list_worktrees(project.service_id())?;

	if leases.is_empty() && worktrees.is_empty() {
		return Ok(());
	}

	clear_stale_terminal_local_worktree_mappings(project, state_store, &leases, &mut worktrees)?;

	if leases.is_empty() && worktrees.is_empty() {
		return Ok(());
	}

	let mut issue_ids = HashSet::new();

	for lease in &leases {
		issue_ids.insert(lease.issue_id().to_owned());
	}
	for mapping in &worktrees {
		issue_ids.insert(mapping.issue_id().to_owned());
	}

	let refreshed_issues = tracker.refresh_issues(&issue_ids.into_iter().collect::<Vec<_>>())?;
	let issues_by_id = refreshed_issues
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect::<HashMap<_, _>>();
	let reconciliation_context = ProjectStateReconciliationContext {
		tracker,
		project,
		workflow,
		state_store,
		worktree_manager,
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let mut cleared_terminal_lane_issue_ids = HashSet::new();

	reconcile_active_project_leases(
		&reconciliation_context,
		&leases,
		&issues_by_id,
		now_unix_epoch,
		&mut cleared_terminal_lane_issue_ids,
	)?;
	cleanup_missing_orphaned_project_worktree_mappings(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
	)?;
	reconcile_orphaned_active_worktree_runs(
		&reconciliation_context,
		&leases,
		&worktrees,
		&issues_by_id,
		now_unix_epoch,
	)?;
	cleanup_terminal_project_worktrees(
		&reconciliation_context,
		&worktrees,
		&issues_by_id,
		&mut cleared_terminal_lane_issue_ids,
	)?;

	Ok(())
}

fn clear_stale_terminal_local_worktree_mappings(
	project: &ServiceConfig,
	state_store: &StateStore,
	leases: &[IssueLease],
	worktrees: &mut Vec<WorktreeMapping>,
) -> Result<()> {
	let active_issue_ids = leases
		.iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<HashSet<_>>();
	let mut cleared_issue_ids = Vec::new();

	for mapping in worktrees.iter() {
		if !worktree_mapping_is_stale_terminal_local_residue(
			project,
			state_store,
			mapping,
			&active_issue_ids,
		)? {
			continue;
		}

		state_store.clear_worktree(mapping.issue_id())?;

		tracing::info!(
			project_id = project.service_id(),
			issue_id = mapping.issue_id(),
			provenance_source = mapping.provenance().source(),
			"Cleared stale terminal local worktree mapping before tracker refresh."
		);

		cleared_issue_ids.push(mapping.issue_id().to_owned());
	}

	if !cleared_issue_ids.is_empty() {
		worktrees.retain(|mapping| !cleared_issue_ids.iter().any(|issue_id| issue_id == mapping.issue_id()));
	}

	Ok(())
}

fn looks_like_tracker_issue_identifier_key(value: &str) -> bool {
	let Some((prefix, number)) = value.rsplit_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& !number.is_empty()
		&& prefix
			.chars()
			.all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
		&& number.chars().all(|character| character.is_ascii_digit())
}

fn local_run_attempt_status_is_terminal(status: &str) -> bool {
	matches!(
		status,
		"succeeded" | "failed" | "interrupted" | "terminated" | TERMINAL_GUARDED_RUN_STATUS
	)
}

fn reconcile_active_project_leases<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for lease in leases {
		if reconcile_success_retained_review_lease(context, lease, issues_by_id)? {
			continue;
		}
		if reconcile_terminal_retained_closeout_lease(
			context,
			lease,
			issues_by_id,
			now_unix_epoch,
			cleared_terminal_lane_issue_ids,
		)? {
			continue;
		}

		reconcile_stale_project_lease(
			context,
			lease,
			issues_by_id,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	Ok(())
}

fn reconcile_success_retained_review_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<bool>
where
	T: IssueTracker,
{
	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& issue.state.name == context.workflow.frontmatter().tracker().success_state()
		&& retained_review_lease_matches_run(context.state_store, lease)?
	{
		mark_run_attempt_if_active(context.state_store, lease.run_id(), "succeeded")?;

		context.state_store.clear_lease(lease.issue_id())?;

		return Ok(true);
	}

	Ok(false)
}

fn reconcile_terminal_retained_closeout_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<bool>
where
	T: IssueTracker,
{
	let Some(issue) = issues_by_id.get(lease.issue_id()) else {
		return Ok(false);
	};

	if !terminal_issue_keeps_retained_closeout(
		context.tracker,
		issue,
		context.project,
		context.workflow,
		context.state_store,
	)? {
		return Ok(false);
	}
	if retained_closeout_lease_has_fresh_activity(
		lease,
		issue,
		context.project,
		now_unix_epoch,
	)? {
		return Ok(true);
	}

	clear_terminal_lane_labels_once(
		context.tracker,
		context.project,
		issue,
		cleared_terminal_lane_issue_ids,
	)?;
	mark_run_attempt_if_active(context.state_store, lease.run_id(), "interrupted")?;

	context.state_store.clear_lease(lease.issue_id())?;

	Ok(true)
}

fn reconcile_stale_project_lease<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	lease: &IssueLease,
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	let reconciled_status = match issues_by_id.get(lease.issue_id()) {
		Some(issue) if is_terminal_issue(issue, context.workflow) => "terminated",
		Some(_) | None => "interrupted",
	};

	if let Some(issue) = issues_by_id.get(lease.issue_id())
		&& is_terminal_issue(issue, context.workflow)
	{
		clear_terminal_lane_labels_once(
			context.tracker,
			context.project,
			issue,
			cleared_terminal_lane_issue_ids,
		)?;
	}

	mark_run_attempt_if_active(context.state_store, lease.run_id(), reconciled_status)?;

	context.state_store.clear_lease(lease.issue_id())
}

fn cleanup_missing_orphaned_project_worktree_mappings<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
) -> Result<()>
where
	T: IssueTracker,
{
	let leased_issue_ids = leases.iter().map(IssueLease::issue_id).collect::<HashSet<_>>();

	for mapping in worktrees {
		if leased_issue_ids.contains(mapping.issue_id())
			|| mapping.provenance().is_legacy_unknown()
			|| !worktree_mapping_path_is_missing(mapping.worktree_path())
		{
			continue;
		}

		let Some(issue) = issues_by_id.get(mapping.issue_id()) else {
			continue;
		};

		if issue_has_service_ownership(context.tracker, issue, context.project.service_id())?
			|| issue.has_label(context.workflow.frontmatter().tracker().needs_attention_label())
			|| context
				.state_store
				.issue_has_active_shared_claim(context.project.service_id(), &issue.id)?
			|| issue_has_running_attempt(context.state_store, &issue.id)?
			|| context
				.state_store
				.review_handoff_marker(
					context.project.service_id(),
					mapping.issue_id(),
					mapping.branch_name(),
				)?
				.is_some()
		{
			continue;
		}

		context.state_store.clear_worktree(mapping.issue_id())?;
	}

	Ok(())
}

fn worktree_mapping_path_is_missing(worktree_path: &Path) -> bool {
	matches!(worktree_path.try_exists(), Ok(false))
}

fn issue_has_running_attempt(state_store: &StateStore, issue_id: &str) -> Result<bool> {
	Ok(state_store
		.latest_run_attempt_for_issue(issue_id)?
		.is_some_and(|attempt| matches!(attempt.status(), "starting" | "running")))
}

fn reconcile_orphaned_active_worktree_runs<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	leases: &[IssueLease],
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
	now_unix_epoch: i64,
) -> Result<()>
where
	T: IssueTracker,
{
	let mut orphaned_actions = Vec::new();

	for mapping in worktrees {
		if leases.iter().any(|lease| lease.issue_id() == mapping.issue_id()) {
			continue;
		}

		let Some(issue) = issues_by_id.get(mapping.issue_id()) else {
			continue;
		};
		let Some(action) = inspect_orphaned_active_worktree_reconciliation(
			context,
			issue,
			mapping,
			now_unix_epoch,
		)? else {
			continue;
		};

		orphaned_actions.push(action);
	}

	apply_run_lease_reconciliation(
		context.tracker,
		context.project,
		context.state_store,
		context.worktree_manager,
		orphaned_actions,
	)
}

fn cleanup_terminal_project_worktrees<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	worktrees: &[WorktreeMapping],
	issues_by_id: &HashMap<String, TrackerIssue>,
	cleared_terminal_lane_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	for mapping in worktrees {
		if let Some(issue) = issues_by_id.get(mapping.issue_id())
			&& is_terminal_issue(issue, context.workflow)
			&& !terminal_issue_keeps_retained_closeout(
				context.tracker,
				issue,
				context.project,
				context.workflow,
				context.state_store,
			)?
		{
			clear_terminal_lane_labels_once(
				context.tracker,
				context.project,
				issue,
				cleared_terminal_lane_issue_ids,
			)?;
			cleanup_worktree_mapping(
				context.state_store,
				context.worktree_manager,
				context.workflow,
				&issue.identifier,
				mapping,
			)?;
		}
	}

	Ok(())
}

fn inspect_orphaned_active_worktree_reconciliation<T>(
	context: &ProjectStateReconciliationContext<'_, T>,
	issue: &TrackerIssue,
	worktree_mapping: &WorktreeMapping,
	now_unix_epoch: i64,
) -> Result<Option<RunLeaseReconciliation>>
where
	T: IssueTracker,
{
	let has_service_ownership =
		issue_has_service_ownership(context.tracker, issue, context.project.service_id())?;
	let needs_attention = issue
		.has_label(context.workflow.frontmatter().tracker().needs_attention_label());

	if !has_service_ownership && !needs_attention {
		return Ok(None);
	}

	let Some(run_attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? else {
		return Ok(None);
	};
	let Some(idle_for) =
		orphaned_run_lease_idle_duration(
			context.state_store,
			&run_attempt,
			worktree_mapping,
			now_unix_epoch,
		)?
	else {
		return Ok(None);
	};
	let disposition = if needs_attention {
		RunLeaseDisposition::StalledAlreadyNeedsAttention { idle_for }
	} else if is_issue_in_progress_for_run(issue, context.workflow)
		&& worktree_has_tracked_changes(worktree_mapping.worktree_path())
	{
		RunLeaseDisposition::StalledRetainedPartialProgress { idle_for }
	} else if is_issue_in_progress_for_run(issue, context.workflow) {
		RunLeaseDisposition::Stalled { idle_for }
	} else {
		return Ok(None);
	};

	Ok(Some(RunLeaseReconciliation {
		issue: issue.clone(),
		run_attempt,
		worktree_mapping: Some(worktree_mapping.clone()),
		disposition,
		workflow: context.workflow.clone(),
	}))
}

fn orphaned_run_lease_idle_duration(
	state_store: &StateStore,
	run_attempt: &RunAttempt,
	worktree_mapping: &WorktreeMapping,
	now_unix_epoch: i64,
) -> Result<Option<Duration>> {
	if !matches!(run_attempt.status(), "starting" | "running") {
		return Ok(None);
	}

	let marker = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
		.filter(|marker| {
			marker.run_id() == run_attempt.run_id()
				&& marker.attempt_number() == run_attempt.attempt_number()
		});

	if let Some(marker) = marker.as_ref()
		&& marker.process_id().is_some()
	{
		if marker_process_is_alive(marker) {
			return Ok(None);
		}

		return Ok(Some(
			marker
				.last_activity_unix_epoch()
				.and_then(|last_activity| observed_idle_duration(last_activity, now_unix_epoch))
				.unwrap_or(Duration::ZERO),
		));
	}

	stalled_idle_duration(
		state_store,
		run_attempt,
		Some(worktree_mapping),
		now_unix_epoch,
	)
}

fn clear_terminal_lane_labels_once<T>(
	tracker: &T,
	project: &ServiceConfig,
	issue: &TrackerIssue,
	cleared_issue_ids: &mut HashSet<String>,
) -> Result<()>
where
	T: IssueTracker,
{
	if cleared_issue_ids.insert(issue.id.clone()) {
		tracker::clear_automation_lane_labels(tracker, issue, project.service_id())?;
	}

	Ok(())
}

fn retained_review_lease_matches_run(
	state_store: &StateStore,
	lease: &IssueLease,
) -> Result<bool> {
	let Some(run_attempt) = state_store.run_attempt(lease.run_id())? else {
		return Ok(false);
	};
	let worktree_mapping = state_store.worktree_for_issue(lease.issue_id())?;

	retained_review_handoff_matches_run(state_store, &run_attempt, worktree_mapping.as_ref())
}

fn terminal_issue_keeps_retained_closeout<T>(
	tracker: &T,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> Result<bool>
where
	T: IssueTracker + ?Sized,
{
	if !is_terminal_issue(issue, workflow) {
		return Ok(false);
	}

	Ok(
		issue_passes_closeout_dispatch_policy(tracker, issue, project, workflow, state_store)?
			|| closeout_dispatch_block_reason(tracker, issue, project, workflow, state_store)?
				.is_some(),
	)
}

fn retained_closeout_lease_has_fresh_activity(
	lease: &IssueLease,
	issue: &TrackerIssue,
	project: &ServiceConfig,
	now_unix_epoch: i64,
) -> Result<bool> {
	let worktree_manager =
		WorktreeManager::new(project.service_id(), project.repo_root(), project.worktree_root());
	let worktree = worktree_manager.plan_for_issue(&issue.identifier);
	let Some(marker) = state::read_run_activity_marker_snapshot(&worktree.path)? else {
		return Ok(false);
	};

	Ok(marker.run_id() == lease.run_id() && worktree_activity_marker_is_fresh(&marker, now_unix_epoch))
}
