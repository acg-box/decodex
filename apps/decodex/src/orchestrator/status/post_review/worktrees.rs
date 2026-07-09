use crate::{
	orchestrator::status::{
		post_review,
		post_review::{
			HashMap, IssueTracker, OperatorPostReviewLaneStatus, OperatorStatusSnapshot,
			PostReviewLaneClassification, PostReviewReadbackDegradation,
			PullRequestReviewStateInspector, ServiceConfig, StateStore, TrackerIssue,
			WorkflowDocument, WorktreeMapping,
		},
	},
	prelude::{Result, eyre},
	state::ReviewLifecycleRecord,
};

pub(crate) fn build_post_review_lane_statuses<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	post_review::build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

pub(crate) fn build_post_review_lane_statuses_and_hydrate_worktrees<T, I>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	review_state_inspector: &I,
	snapshot: &mut OperatorStatusSnapshot,
) -> Result<Vec<OperatorPostReviewLaneStatus>>
where
	T: IssueTracker,
	I: PullRequestReviewStateInspector,
{
	let worktree_issues = load_post_review_worktree_issues(tracker, project, state_store)?;

	post_review::hydrate_worktree_issue_metadata(snapshot, &worktree_issues);

	post_review::build_post_review_lane_statuses_from_worktree_issues(
		project,
		workflow,
		state_store,
		review_state_inspector,
		worktree_issues,
	)
}

pub(crate) fn load_post_review_worktree_issues<T>(
	tracker: &T,
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<Vec<(WorktreeMapping, TrackerIssue)>>
where
	T: IssueTracker,
{
	let active_issue_ids = post_review::active_shared_issue_ids(project, state_store)?;
	let worktrees = state_store
		.list_worktrees(project.service_id())?
		.into_iter()
		.filter_map(|mapping| {
			match post_review::worktree_mapping_is_stale_terminal_local_residue(
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
		return Ok(Vec::new());
	}

	let issue_ids =
		worktrees.iter().map(|mapping| mapping.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = post_review::refresh_recoverable_runtime_issues(tracker, &issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();

	Ok(worktrees
		.into_iter()
		.filter_map(|worktree| {
			issues_by_id.get(worktree.issue_id()).cloned().map(|issue| (worktree, issue))
		})
		.collect())
}

pub(crate) fn build_degraded_post_review_lane_statuses<I>(
	project: &ServiceConfig,
	state_store: &StateStore,
	review_state_inspector: &I,
) -> Result<Vec<OperatorPostReviewLaneStatus>>
where
	I: PullRequestReviewStateInspector,
{
	let mut lanes = Vec::new();

	for worktree in state_store.list_worktrees(project.service_id())? {
		let Some(lifecycle_record) = state_store.review_lifecycle_record(
			project.service_id(),
			worktree.issue_id(),
			worktree.branch_name(),
		)?
		else {
			continue;
		};

		if lifecycle_record.target_base_ref_name().is_none() {
			return Err(eyre::eyre!(
				"Degraded post-review status requires lifecycle authority for `{}` on branch `{}` to include the PR base branch.",
				worktree.issue_id(),
				worktree.branch_name()
			));
		}

		let issue_identifier = retained_issue_identifier_from_worktree(&worktree);
		let review_state = review_state_inspector
			.inspect_review_state(worktree.worktree_path(), lifecycle_record.pr_url())
			.ok();
		let classification =
			PostReviewReadbackDegradation::tracker_issue_from_lifecycle(&lifecycle_record)
				.wait_for_review_classification(review_state);

		lanes.push(degraded_post_review_lane_status_from_classification(
			project,
			state_store,
			&worktree,
			&lifecycle_record,
			issue_identifier,
			classification,
		)?);
	}

	lanes.sort_by(|left, right| left.issue_identifier.cmp(&right.issue_identifier));

	Ok(lanes)
}

pub(crate) fn degraded_post_review_lane_status_from_classification(
	project: &ServiceConfig,
	state_store: &StateStore,
	worktree: &WorktreeMapping,
	lifecycle_record: &ReviewLifecycleRecord,
	issue_identifier: String,
	classification: PostReviewLaneClassification,
) -> Result<OperatorPostReviewLaneStatus> {
	let loop_status = post_review::operator_loop_status_for_run(
		project,
		state_store,
		worktree.issue_id(),
		lifecycle_record.run_id(),
		lifecycle_record.attempt_number(),
		Some("repair"),
		None,
	)?;

	Ok(OperatorPostReviewLaneStatus {
		project_id: project.service_id().to_owned(),
		issue_id: worktree.issue_id().to_owned(),
		issue_identifier,
		issue_state: String::from("tracker_readback_degraded"),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: post_review::relative_worktree_path_for_path(
			project,
			worktree.worktree_path(),
		),
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

pub(crate) fn retained_issue_identifier_from_worktree(worktree: &WorktreeMapping) -> String {
	worktree
		.worktree_path()
		.file_name()
		.and_then(|name| name.to_str())
		.map(str::trim)
		.filter(|name| !name.is_empty())
		.unwrap_or_else(|| worktree.issue_id())
		.to_ascii_uppercase()
}
