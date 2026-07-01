#[allow(clippy::wildcard_imports)] use super::*;

use state::WORKTREE_PROVENANCE_RUNTIME_RECORDED;

mod admin_merge;
mod attention;
mod markers;
mod phases;

#[allow(clippy::wildcard_imports)] use admin_merge::*;
#[cfg(test)]
pub(crate) use attention::apply_passive_retained_manual_attention_with_run_identity;
#[allow(clippy::wildcard_imports)] use attention::*;
#[cfg(test)] pub(crate) use markers::ensure_review_orchestration_marker;
#[allow(clippy::wildcard_imports)] use markers::*;
#[allow(clippy::wildcard_imports)] use phases::*;

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

pub(crate) struct PassiveRetainedAttentionRuntime<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
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

pub(crate) fn reconcile_post_review_orchestration<T>(
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

pub(crate) fn reconcile_post_review_orchestration_with_inspector<T, I>(
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
