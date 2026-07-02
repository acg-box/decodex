use std::{
	collections::{HashMap, HashSet},
	path::Path,
};

use time::OffsetDateTime;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		GhPullRequestReviewStateInspector, PullRequestReviewStateInspector, RetainedReviewLaneLoad,
		retained_review_orchestration::{
			attention, load, model::PassiveRetainedAttentionRuntime, phases, stale_worktree,
		},
		status,
	},
	prelude::Result,
	state::StateStore,
	tracker::IssueTracker,
	workflow::WorkflowDocument,
};

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
		.filter_map(
			|mapping| match stale_worktree::worktree_mapping_is_stale_terminal_local_residue(
				project,
				state_store,
				&mapping,
				&active_issue_ids,
			) {
				Ok(true) => None,
				Ok(false) => Some(Ok(mapping)),
				Err(error) => Some(Err(error)),
			},
		)
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

		if !load::eligible_post_review_orchestration_issue(
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

		let lane = match load::load_retained_review_lane(
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
				attention::apply_passive_retained_manual_attention_with_run_identity(
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

		if let Some(reason) = status::validate_review_orchestration_marker(
			&lane.snapshot,
			&lane.review_state,
			&lane.orchestration_marker,
		) {
			attention::apply_passive_retained_manual_attention(
				PassiveRetainedAttentionRuntime { tracker, project, workflow, state_store },
				&lane.snapshot.issue,
				&lane.snapshot.worktree,
				&lane.orchestration_marker,
				reason,
			)?;

			continue;
		}

		phases::reconcile_retained_review_lane(
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
