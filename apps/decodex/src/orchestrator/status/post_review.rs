use super::{
	AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE, Command, HashMap,
	IssueTracker, OffsetDateTime, OperatorLoopStatus, OperatorPostReviewLaneStatus,
	OperatorStatusSnapshot, Path, PostReviewLaneBuildContext, PostReviewLaneClassification,
	PostReviewLaneDecision, PostReviewLaneSnapshot, PostReviewLaneStateLoad,
	PostReviewOrchestrationStatus, PostReviewReadbackDegradation, PostReviewRuntimeState,
	PrivateExecutionEvent, PullRequestMergeViewResponse, PullRequestReadbackRootCause,
	PullRequestReviewState, PullRequestReviewStateInspector, ReviewHandoffMarker, ServiceConfig,
	StateStore, TrackerIssue, Value, WorkflowDocument, WorktreeMapping, active_shared_issue_ids,
	apply_non_github_review_post_review_classification,
	apply_pre_orchestration_post_review_classification,
	apply_review_orchestration_phase_classification, blocked_post_review_lane_status,
	classify_pull_request_readback_report, github, initial_post_review_lane_classification,
	issue_retry_budget_exhausted_for_worktree, load_post_review_lane_review_state,
	load_post_review_orchestration_marker, operator_boundary_policy_blocks_landing,
	operator_boundary_policy_requires_enhanced_evidence, operator_loop_status_for_run,
	refresh_recoverable_runtime_issues, relative_worktree_path_for_path,
	resolve_configured_env_var, tracker, worktree_checkout_branch_name, worktree_head_oid,
	worktree_mapping_is_stale_terminal_local_residue,
};

#[cfg(test)] use super::ReviewLevel;

pub(in crate::orchestrator) fn build_post_review_lane_statuses<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

pub(in crate::orchestrator) fn build_post_review_lane_statuses_and_hydrate_worktrees<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	snapshot: &mut OperatorStatusSnapshot,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	hydrate_worktree_issue_metadata(snapshot, &worktree_issues);

	build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

pub(in crate::orchestrator) fn load_post_review_worktree_issues<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<Vec<(WorktreeMapping, TrackerIssue)>>
where
	T: IssueTracker,
{
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
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
		.collect::<crate::prelude::Result<Vec<_>>>()?;

	if worktrees.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids =
		worktrees.iter().map(|mapping| mapping.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = refresh_recoverable_runtime_issues(tracker, &issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	Ok(worktrees
		.into_iter()
		.filter_map(|worktree| {
			issues_by_id.get(worktree.issue_id()).cloned().map(|issue| (worktree, issue))
		})
		.collect())
}

pub(in crate::orchestrator) fn build_degraded_post_review_lane_statuses<I>(
	project: &ServiceConfig,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let mut lanes = Vec::new();

	for worktree in state_store.list_worktrees(project.service_id())? {
		let Some(review_handoff) = state_store.review_handoff_marker(
			project.service_id(),
			worktree.issue_id(),
			worktree.branch_name(),
		)?
		else {
			continue;
		};
		let issue_identifier = retained_issue_identifier_from_worktree(&worktree);
		let review_state = review_state_inspector
			.inspect_review_state(worktree.worktree_path(), review_handoff.pr_url())
			.ok();
		let classification =
			PostReviewReadbackDegradation::tracker_issue_from_handoff(&review_handoff)
				.wait_for_review_classification(review_state);

		lanes.push(degraded_post_review_lane_status_from_classification(
			project,
			state_store,
			&worktree,
			&review_handoff,
			issue_identifier,
			classification,
		)?);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

pub(in crate::orchestrator) fn degraded_post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree: &WorktreeMapping,
	review_handoff: &ReviewHandoffMarker,
	issue_identifier: String,
	classification: PostReviewLaneClassification,
) -> crate::prelude::Result<OperatorPostReviewLaneStatus> {
	let loop_status = operator_loop_status_for_run(
		project,
		state_store,
		worktree.issue_id(),
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		Some("repair"),
		None,
	)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: worktree.issue_id().to_owned(),
		issue_identifier,
		issue_state: String::from("tracker_readback_degraded"),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, worktree.worktree_path()),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		shadowed_by_current_lane: false,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status: Some(loop_status),
	})
}

pub(in crate::orchestrator) fn retained_issue_identifier_from_worktree(
	worktree: &WorktreeMapping,
) -> String {
	worktree
		.worktree_path()
		.file_name()
		.and_then(|name| name.to_str())
		.map(str::trim)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| worktree.issue_id())
		.to_ascii_uppercase()
}

pub(in crate::orchestrator) fn build_post_review_lane_statuses_from_worktree_issues<I>(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	worktree_issues: Vec<(WorktreeMapping, TrackerIssue)>,
) -> crate::prelude::Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let tracker_policy = workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let completed_state = tracker_policy.resolved_completed_state();
	let lane_context = PostReviewLaneBuildContext {
		project,
		workflow,
		state_store,
		review_state_inspector,
		success_state,
		completed_state,
	};
	let mut lanes = Vec::new();

	for (worktree, issue) in worktree_issues {
		let Some(lane) = build_post_review_lane_status(&lane_context, issue, worktree)? else {
			continue;
		};

		lanes.push(lane);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

pub(in crate::orchestrator) fn hydrate_worktree_issue_metadata(
	snapshot: &mut OperatorStatusSnapshot,
	worktree_issues: &[(WorktreeMapping, TrackerIssue)],
) {
	let issues_by_id = worktree_issues
		.iter()
		.map(|(_, issue)| (issue.id.as_str(), issue))
		.collect::<HashMap<_, _>>();

	for worktree in &mut snapshot.worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id.as_str()) else {
			continue;
		};

		worktree.issue_identifier = Some(issue.identifier.clone());
		worktree.issue_state = Some(issue.state.name.clone());
	}
}

pub(in crate::orchestrator) fn build_post_review_lane_status<I>(
	context: &PostReviewLaneBuildContext<'_, I>,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
) -> crate::prelude::Result<Option<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	if issue.state.name != context.success_state && issue.state.name != context.completed_state {
		return Ok(None);
	}

	if let Some(reason) = post_review_lane_static_block_reason(&issue, context.workflow)? {
		return Ok(Some(blocked_post_review_lane_status(
			context.project,
			&issue,
			&worktree,
			reason,
		)));
	}

	let retry_budget_exhausted = issue_retry_budget_exhausted_for_worktree(
		context.workflow,
		context.state_store,
		&issue.id,
		worktree.worktree_path(),
	)?;
	let review_handoff = context.state_store.review_handoff_marker(
		context.project.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;

	if issue.state.name == context.completed_state && review_handoff.is_none() {
		return Ok(None);
	}

	let local_branch_name = match worktree_checkout_branch_name(worktree.worktree_path()) {
		Ok(local_branch_name) => local_branch_name,
		Err(_error) => {
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_checkout_branch_read_failed",
			)));
		},
	};
	let local_head_oid = match worktree_head_oid(worktree.worktree_path()) {
		Ok(local_head_oid) => local_head_oid,
		Err(_error) => {
			return Ok(Some(blocked_post_review_lane_status(
				context.project,
				&issue,
				&worktree,
				"worktree_head_read_failed",
			)));
		},
	};
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff,
		local_branch_name,
		local_head_oid,
	};
	let mut classification = classify_post_review_lane_with_project(
		&snapshot,
		context.project,
		context.workflow,
		context.state_store,
		context.review_state_inspector,
	)?;

	if retry_budget_exhausted {
		classification = retry_budget_exhausted_post_review_lane_classification(
			&snapshot,
			context.project,
			context.workflow,
			context.review_state_inspector,
			classification,
		);
	}

	apply_active_ownership_warning_to_post_review_lane(
		context.project,
		context.success_state,
		&snapshot,
		&mut classification,
	);

	Ok(Some(post_review_lane_status_from_classification(
		context.project,
		context.state_store,
		&snapshot,
		classification,
	)?))
}

pub(in crate::orchestrator) fn apply_active_ownership_warning_to_post_review_lane(
	project: &ServiceConfig,
	success_state: &str,
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
) {
	if snapshot.review_handoff.is_none()
		|| snapshot.issue.state.name != success_state
		|| !snapshot.issue.labels_complete
		|| snapshot.issue.has_label(&tracker::automation_active_label(project.service_id()))
	{
		return;
	}
	if classification.readback_warning.is_none() {
		classification.readback_warning = Some(String::from("active_ownership_label_missing"));
	}
}

pub(in crate::orchestrator) fn post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	classification: PostReviewLaneClassification,
) -> crate::prelude::Result<OperatorPostReviewLaneStatus> {
	let loop_status =
		operator_post_review_loop_status(project, state_store, snapshot, classification.decision)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: snapshot.issue.id.clone(),
		issue_identifier: snapshot.issue.identifier.clone(),
		issue_state: snapshot.issue.state.name.clone(),
		branch_name: snapshot.worktree.branch_name().to_owned(),
		worktree_path: relative_worktree_path_for_path(project, snapshot.worktree.worktree_path()),
		classification: classification.decision.as_str().to_owned(),
		reason: classification.reason,
		pr_url: classification.pr_url,
		pr_head_sha: classification.pr_head_sha,
		pr_state: classification.pr_state,
		review_decision: classification.review_decision,
		mergeable: classification.mergeable,
		check_state: classification.check_state,
		unresolved_review_threads: classification.unresolved_review_threads,
		shadowed_by_current_lane: false,
		readback_warning: classification.readback_warning,
		readback_root_cause: classification.readback_root_cause,
		loop_status,
	})
}

pub(in crate::orchestrator) fn operator_post_review_loop_status(
	project: &ServiceConfig,
	state_store: &StateStore,
	snapshot: &PostReviewLaneSnapshot,
	decision: PostReviewLaneDecision,
) -> crate::prelude::Result<Option<OperatorLoopStatus>> {
	let Some(review_handoff) = snapshot.review_handoff.as_ref() else {
		return Ok(None);
	};
	let default_review_phase = match decision {
		PostReviewLaneDecision::ReadyToLand | PostReviewLaneDecision::WaitForReview => None,
		_ => Some("repair"),
	};

	operator_loop_status_for_run(
		project,
		state_store,
		&snapshot.issue.id,
		review_handoff.run_id(),
		review_handoff.attempt_number(),
		default_review_phase,
		None,
	)
	.map(Some)
}

pub(in crate::orchestrator) fn post_review_lane_static_block_reason(
	issue: &TrackerIssue,
	workflow: &WorkflowDocument,
) -> crate::prelude::Result<Option<&'static str>> {
	let tracker_policy = workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		return Ok(Some("issue_opted_out"));
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		return Ok(Some("issue_needs_attention"));
	}

	Ok(None)
}

#[cfg_attr(not(test), allow(dead_code))]
#[cfg(test)]
pub(in crate::orchestrator) fn classify_post_review_lane<I>(
	snapshot: &PostReviewLaneSnapshot,
	state_store: &StateStore,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		true,
		Some(PostReviewRuntimeState {
			state_store,
			project_id: "pubfi",
			review_level: ReviewLevel::Standard,
		}),
	)
}

pub(in crate::orchestrator) fn classify_post_review_lane_with_project<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let mut classification = classify_post_review_lane_with_external_review(
		snapshot,
		workflow,
		review_state_inspector,
		project.codex().review_level().uses_github_review(),
		Some(PostReviewRuntimeState {
			state_store,
			project_id: project.service_id(),
			review_level: project.codex().review_level(),
		}),
	)?;

	confirm_status_visible_merged_closeout(snapshot, project, &mut classification);

	Ok(classification)
}

pub(in crate::orchestrator) fn classify_post_review_lane_with_external_review<I>(
	snapshot: &PostReviewLaneSnapshot,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	github_review_enabled: bool,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<PostReviewLaneClassification>
where
	I: PullRequestReviewStateInspector,
{
	let review_state = match load_post_review_lane_review_state(snapshot, review_state_inspector)? {
		PostReviewLaneStateLoad::Classification(classification) => return Ok(classification),
		PostReviewLaneStateLoad::ReviewState(review_state) => review_state,
	};
	let mut classification = initial_post_review_lane_classification(&review_state);

	if apply_pre_orchestration_post_review_classification(
		snapshot,
		workflow,
		&review_state,
		&mut classification,
	) {
		return Ok(classification);
	}
	if !github_review_enabled {
		let orchestration_marker = load_post_review_orchestration_marker(
			snapshot,
			&review_state,
			&mut classification,
			runtime_state,
		)?;

		if classification.decision == PostReviewLaneDecision::Block {
			return Ok(classification);
		}

		apply_non_github_review_post_review_classification(
			&mut classification,
			&review_state,
			orchestration_marker.as_ref(),
			OffsetDateTime::now_utc().unix_timestamp(),
		)?;
		apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

		return Ok(classification);
	}

	let Some(orchestration_marker) = load_post_review_orchestration_marker(
		snapshot,
		&review_state,
		&mut classification,
		runtime_state,
	)?
	else {
		return Ok(classification);
	};
	let orchestration_status =
		PostReviewOrchestrationStatus::from_review_state(&review_state, &orchestration_marker)?;

	apply_review_orchestration_phase_classification(
		&mut classification,
		&review_state,
		&orchestration_marker,
		&orchestration_status,
		OffsetDateTime::now_utc().unix_timestamp(),
	);
	apply_authority_boundary_landing_policy(snapshot, &mut classification, runtime_state)?;

	Ok(classification)
}

pub(in crate::orchestrator) fn apply_authority_boundary_landing_policy(
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<()> {
	if classification.decision != PostReviewLaneDecision::ReadyToLand {
		return Ok(());
	}

	let Some(reason) = authority_boundary_landing_requirement(snapshot, runtime_state)? else {
		return Ok(());
	};

	classification.decision = if reason == "authority_boundary_requires_human_decision" {
		PostReviewLaneDecision::Block
	} else {
		PostReviewLaneDecision::NeedsReviewRepair
	};
	classification.reason = reason.to_owned();

	Ok(())
}

pub(in crate::orchestrator) fn authority_boundary_landing_requirement(
	snapshot: &PostReviewLaneSnapshot,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> crate::prelude::Result<Option<&'static str>> {
	let Some(runtime_state) = runtime_state else {
		return Ok(None);
	};
	let events = runtime_state
		.state_store
		.list_private_execution_events_for_issue(runtime_state.project_id, &snapshot.issue.id)?;

	if events.iter().any(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}
	if events.iter().rev().any(|event| {
		event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
			&& authority_boundary_event_requires_human_decision(event.payload())
	}) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}

	let latest_clean_review_record_id = events
		.iter()
		.rev()
		.find(|event| authority_boundary_clearance_review_checkpoint(event, snapshot))
		.map_or(0, PrivateExecutionEvent::record_id);

	for event in events.iter().rev() {
		if event.record_id() <= latest_clean_review_record_id
			|| event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
		{
			continue;
		}

		if let Some(reason) = authority_boundary_event_landing_requirement(event.payload()) {
			return Ok(Some(reason));
		}
	}

	Ok(None)
}

pub(in crate::orchestrator) fn authority_boundary_clearance_review_checkpoint(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
) -> bool {
	if event.event_type() != "review_checkpoint"
		|| event.payload().get("status").and_then(Value::as_str) != Some("clean")
	{
		return false;
	}

	let Some(checkpoint_head) = event.payload().get("head_sha").and_then(Value::as_str) else {
		return false;
	};
	let expected_head = snapshot
		.local_head_oid
		.as_deref()
		.or_else(|| snapshot.review_handoff.as_ref().map(ReviewHandoffMarker::pr_head_oid));

	expected_head == Some(checkpoint_head)
}

pub(in crate::orchestrator) fn authority_boundary_event_blocks_landing(payload: &Value) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("blocks_landing").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_blocks_landing)
		})
}

pub(in crate::orchestrator) fn authority_boundary_event_requires_enhanced_evidence(
	payload: &Value,
) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("requires_enhanced_evidence").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		})
}

pub(in crate::orchestrator) fn authority_boundary_event_landing_requirement(
	payload: &Value,
) -> Option<&'static str> {
	if authority_boundary_event_blocks_landing(payload) {
		return Some("authority_boundary_blocks_landing");
	}
	if authority_boundary_event_requires_enhanced_evidence(payload) {
		return Some("authority_boundary_requires_enhanced_evidence");
	}

	None
}

pub(in crate::orchestrator) fn authority_boundary_event_requires_human_decision(
	payload: &Value,
) -> bool {
	authority_boundary_event_policy_decision(payload)
		.is_some_and(|policy_decision| policy_decision == "requires_human_decision")
		|| payload
			.get("policy")
			.and_then(|policy| policy.get("requires_human_decision"))
			.and_then(Value::as_bool)
			.unwrap_or(false)
		|| matches!(
			payload.get("disposition").and_then(Value::as_str).or_else(|| {
				payload
					.get("final_disposition")
					.and_then(|final_disposition| final_disposition.get("disposition"))
					.and_then(Value::as_str)
			}),
			Some("requires_human" | "insufficient_evidence")
		)
}

pub(in crate::orchestrator) fn authority_boundary_event_policy_decision(
	payload: &Value,
) -> Option<&str> {
	payload.get("policy_decision").and_then(Value::as_str).or_else(|| {
		payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
	})
}

pub(in crate::orchestrator) fn retry_budget_exhausted_post_review_lane_classification<I>(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	review_state_inspector: &I,
	mut classification: PostReviewLaneClassification,
) -> PostReviewLaneClassification
where
	I: PullRequestReviewStateInspector,
{
	if classification.pr_url.is_none() {
		classification.pr_url =
			snapshot.review_handoff.as_ref().map(|marker| marker.pr_url().to_owned());
	}
	if classification.pr_state.is_none()
		&& let Some(review_state) =
			retry_budget_exhausted_merged_review_state(snapshot, review_state_inspector)
	{
		classification = initial_post_review_lane_classification(&review_state);

		apply_pre_orchestration_post_review_classification(
			snapshot,
			workflow,
			&review_state,
			&mut classification,
		);
	}
	if merged_closeout_pending_classification(&classification)
		&& worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		if snapshot.issue.state.name == workflow.frontmatter().tracker().resolved_completed_state()
			&& !worktree_has_no_tracked_changes(project.repo_root())
		{
			classification.decision = PostReviewLaneDecision::CleanupBlocked;
			classification.reason = String::from("default_branch_worktree_dirty");

			return classification;
		}

		return classification;
	}
	if classification.pr_state.as_deref() == Some("MERGED")
		&& worktree_has_no_tracked_changes(snapshot.worktree.worktree_path())
	{
		classification.decision = if snapshot.issue.state.name
			== workflow.frontmatter().tracker().resolved_completed_state()
		{
			PostReviewLaneDecision::CleanupBlocked
		} else {
			PostReviewLaneDecision::CloseoutBlocked
		};
		classification.reason = String::from("retry_budget_exhausted");

		return classification;
	}

	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("retry_budget_exhausted");

	classification
}

pub(in crate::orchestrator) fn merged_closeout_pending_classification(
	classification: &PostReviewLaneClassification,
) -> bool {
	classification.decision == PostReviewLaneDecision::Continue
		&& classification.reason == "pull_request_merged_closeout_pending"
		&& classification.pr_state.as_deref() == Some("MERGED")
}

pub(in crate::orchestrator) fn confirm_status_visible_merged_closeout(
	snapshot: &PostReviewLaneSnapshot,
	project: &ServiceConfig,
	classification: &mut PostReviewLaneClassification,
) {
	if !merged_closeout_pending_classification(classification) {
		return;
	}

	let Some(pr_url) = classification.pr_url.as_deref() else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let expected_head_sha = snapshot
		.review_handoff
		.as_ref()
		.map(ReviewHandoffMarker::pr_head_oid)
		.or(classification.pr_head_sha.as_deref());
	let Some(expected_head_sha) = expected_head_sha else {
		mark_merged_closeout_confirmation_conflict(classification, None, None);

		return;
	};
	let github_token = match resolve_configured_env_var(
		"github.token_env_var",
		Some(project.github().token_env_var()),
	) {
		Ok(github_token) => github_token,
		Err(error) => {
			let root_cause = classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};
	let merge_readback = match github::inspect_pull_request_merge_readback(
		snapshot.worktree.worktree_path(),
		pr_url,
		&github_token,
		project.github().command_path(),
	) {
		Ok(merge_readback) => merge_readback,
		Err(error) => {
			let root_cause = classify_pull_request_readback_report(&error);

			mark_merged_closeout_confirmation_conflict(classification, None, Some(root_cause));

			return;
		},
	};

	if merge_readback.state == "MERGED"
		&& merge_readback.head_ref_oid.as_deref() == Some(expected_head_sha)
	{
		return;
	}

	mark_merged_closeout_confirmation_conflict(
		classification,
		Some(merge_readback),
		Some(PullRequestReadbackRootCause::LineageValidationFailed),
	);
}

pub(in crate::orchestrator) fn mark_merged_closeout_confirmation_conflict(
	classification: &mut PostReviewLaneClassification,
	merge_readback: Option<PullRequestMergeViewResponse>,
	root_cause: Option<PullRequestReadbackRootCause>,
) {
	classification.decision = PostReviewLaneDecision::Block;
	classification.reason = String::from("pull_request_merge_state_conflict");
	classification.readback_warning = Some(String::from("pull_request_merge_state_conflict"));
	classification.readback_root_cause =
		root_cause.map(|root_cause| root_cause.as_str().to_owned());

	if let Some(merge_readback) = merge_readback {
		classification.pr_state = Some(merge_readback.state);
		classification.pr_head_sha =
			merge_readback.head_ref_oid.or_else(|| classification.pr_head_sha.clone());
	}
}

pub(in crate::orchestrator) fn retry_budget_exhausted_merged_review_state<I>(
	snapshot: &PostReviewLaneSnapshot,
	review_state_inspector: &I,
) -> Option<PullRequestReviewState>
where
	I: PullRequestReviewStateInspector,
{
	let review_handoff = snapshot.review_handoff.as_ref()?;

	if !worktree_has_no_tracked_changes(snapshot.worktree.worktree_path()) {
		return None;
	}

	let review_state = review_state_inspector
		.inspect_review_state(snapshot.worktree.worktree_path(), review_handoff.pr_url())
		.ok()?;

	(review_state.state == "MERGED").then_some(review_state)
}

pub(in crate::orchestrator) fn worktree_has_no_tracked_changes(worktree_path: &Path) -> bool {
	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
	else {
		return false;
	};

	output.status.success() && String::from_utf8_lossy(&output.stdout).trim().is_empty()
}
