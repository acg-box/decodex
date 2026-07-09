use crate::{
	prelude::{Result, eyre},
	recovery::{
		closeout::{
			SupersededCloseoutValidation,
			validation::merged::{issue, pull_request},
		},
		context::RecoveryContext,
		pull_request_inspection,
		requests::SupersededCloseoutRecoveryRequest,
		review_handoff,
	},
	tracker::TrackerIssue,
};

pub(in crate::recovery) fn validate_superseded_closeout_request(
	context: &RecoveryContext,
	request: &SupersededCloseoutRecoveryRequest,
) -> Result<SupersededCloseoutValidation> {
	if request.issue.eq_ignore_ascii_case(&request.successor_issue) {
		eyre::bail!("Superseded issue and successor issue must be distinct.");
	}
	if request.pr_url.trim() == request.successor_pr_url.trim() {
		eyre::bail!("Superseded PR and successor PR must be distinct.");
	}

	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;
	let successor_issue =
		review_handoff::load_issue_by_identifier(&context.tracker, &request.successor_issue)?;
	let completed_state_id = validate_superseded_issue_context(context, &issue)?;
	validate_successor_issue_context(context, &successor_issue)?;
	validate_same_tracker_team(&issue, &successor_issue)?;

	let (obsolete_landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;
	validate_obsolete_pull_request(&obsolete_landing_state, &default_branch)?;

	let (successor_landing_state, successor_default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.successor_pr_url)?;
	if successor_default_branch != default_branch {
		eyre::bail!(
			"Successor PR default branch `{successor_default_branch}` does not match obsolete PR default branch `{default_branch}`."
		);
	}
	pull_request::validate_merged_closeout_pull_request(
		context,
		&successor_landing_state,
		&default_branch,
	)?;

	let successor_merge_commit =
		pull_request_inspection::inspect_project_pull_request_merge_commit(
			context,
			&request.successor_pr_url,
		)?;
	pull_request::ensure_merge_commit_reachable_from_remote_default_branch(
		context.config.repo_root(),
		&request.successor_pr_url,
		&successor_merge_commit,
		&default_branch,
	)?;
	pull_request::ensure_head_has_no_unique_patch_from_remote_default_branch(
		context.config.repo_root(),
		&obsolete_landing_state.head_ref_oid,
		&default_branch,
		"obsolete PR has no unique unlanded patch after the successor PR landed",
	)?;

	let worktree_mapping = issue::retained_worktree_mapping_for_issue(context, &issue)?
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` has no retained worktree mapping; superseded closeout requires the obsolete retained lane mapping.",
				issue.identifier
			)
		})?;
	let local_head = review_handoff::validate_retained_pr_worktree(
		&worktree_mapping,
		&obsolete_landing_state,
		"superseded closeout",
	)?;
	if local_head != obsolete_landing_state.head_ref_oid {
		eyre::bail!(
			"Retained worktree HEAD `{local_head}` does not match obsolete PR head `{}`.",
			obsolete_landing_state.head_ref_oid
		);
	}

	let worktree_path_for_event = review_handoff::relative_worktree_path_for_recovery(
		context,
		worktree_mapping.worktree_path(),
	)
	.unwrap_or_else(|| worktree_mapping.worktree_path().display().to_string());
	let (run_id, attempt_number) =
		if let Some(attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? {
			(attempt.run_id().to_owned(), attempt.attempt_number())
		} else {
			(format!("superseded-closeout-{}", issue.identifier.to_ascii_lowercase()), 1)
		};

	Ok(SupersededCloseoutValidation {
		issue,
		successor_issue,
		branch_name: worktree_mapping.branch_name().to_owned(),
		worktree_path_for_event,
		run_id,
		attempt_number,
		obsolete_landing_state,
		successor_landing_state,
		successor_merge_commit,
		completed_state_id,
	})
}

fn validate_same_tracker_team(issue: &TrackerIssue, successor_issue: &TrackerIssue) -> Result<()> {
	if issue.team.id == successor_issue.team.id {
		return Ok(());
	}

	eyre::bail!(
		"Superseded issue `{}` belongs to team `{}`, but successor issue `{}` belongs to team `{}`.",
		issue.identifier,
		issue.team.name,
		successor_issue.identifier,
		successor_issue.team.name
	)
}

fn validate_superseded_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<String> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	issue
		.team
		.states
		.iter()
		.find(|state| state.name == tracker_policy.resolved_completed_state())
		.map(|state| state.id.clone())
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` team has no completed state `{}`.",
				issue.identifier,
				tracker_policy.resolved_completed_state()
			)
		})
}

fn validate_successor_issue_context(context: &RecoveryContext, issue: &TrackerIssue) -> Result<()> {
	let tracker_policy = context.workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();

	if issue.state.name != completed_state {
		eyre::bail!(
			"Successor issue `{}` is in `{}`, but superseded closeout requires `{completed_state}`.",
			issue.identifier,
			issue.state.name
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Successor issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	Ok(())
}

fn validate_obsolete_pull_request(
	landing_state: &crate::pull_request::PullRequestLandingState,
	default_branch: &str,
) -> Result<()> {
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Obsolete pull request `{}` targets `{}`, but configured default branch is `{default_branch}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state == "MERGED" {
		eyre::bail!(
			"Obsolete pull request `{}` is already merged; use merged closeout recovery for same-PR lineage.",
			pull_request_inspection::landing_url(landing_state)
		);
	}
	if !matches!(landing_state.state.as_str(), "OPEN" | "CLOSED") {
		eyre::bail!(
			"Obsolete pull request `{}` is `{}`; superseded closeout requires `OPEN` or already `CLOSED`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Obsolete pull request `{}` does not expose the retained head branch required for superseded closeout.",
			pull_request_inspection::landing_url(landing_state)
		);
	}

	Ok(())
}
