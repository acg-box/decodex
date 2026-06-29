//! Diagnostic assembly for stale-active recovery.

use std::path::Path;

use crate::{
	commit_message,
	prelude::{Result, eyre},
	state::{self, ProjectRunStatus, StateStore},
	tracker::{self, IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

use super::{
	STALE_ACTIVE_BLOCKED_CLASSIFICATION, STALE_ACTIVE_CLASSIFICATION,
	STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION,
	context::RecoveryRuntimeMutationPolicy,
	process_liveness::stale_active_optional_marker_process_liveness,
	reports::StaleActiveDiagnostic,
	stale_active_authority::{
		inspect_stale_active_private_evidence, inspect_stale_active_review_lineage,
	},
	stale_active_labels::{
		inspect_stale_active_labels, inspect_stale_active_shared_claim,
		stale_active_tracker_issue_keys,
	},
	stale_active_reentry::{
		StaleActiveReleaseReentryInput, apply_stale_active_release_reentries, evidence_contains,
	},
	stale_active_runtime::{
		inspect_stale_active_control_channel, inspect_stale_active_run_evidence,
		latest_stale_active_run, stale_active_runs,
	},
	stale_active_worktree::{
		inspect_stale_active_worktree, stale_active_worktree_mapping_for_keys,
	},
};

pub(super) fn diagnose_stale_active_issues<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<StaleActiveDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	let issues = if let Some(selector) = selector {
		vec![lookup_stale_active_issue(tracker, selector)?]
	} else {
		tracker.list_issues_with_label(&tracker::automation_active_label(project_id))?
	};

	issues
		.into_iter()
		.map(|issue| {
			inspect_stale_active_issue(
				project_id,
				workflow,
				worktree_root,
				state_store,
				tracker,
				issue,
				listing_mode,
			)
		})
		.collect()
}

pub(super) fn lookup_stale_active_issue<T>(tracker: &T, selector: &str) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	let selector = selector.trim();

	if selector.is_empty() {
		eyre::bail!("Issue selector must not be empty.");
	}

	if commit_message::looks_like_issue_identifier(selector) {
		return tracker
			.get_issue_by_identifier(selector)?
			.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."));
	}

	if let Some(issue) = tracker.refresh_issues(&[selector.to_owned()])?.pop() {
		return Ok(issue);
	}

	tracker
		.get_issue_by_identifier(selector)?
		.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."))
}

fn inspect_stale_active_issue<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	issue: TrackerIssue,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut evidence = vec![String::from("tracker_issue_present")];
	let mut blockers = Vec::new();
	let issue_keys = stale_active_tracker_issue_keys(&issue);
	let labels = inspect_stale_active_labels(
		project_id,
		workflow,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;
	let active_shared_claim = inspect_stale_active_shared_claim(
		project_id,
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	);

	let runs = stale_active_runs(project_id, state_store, &issue_keys, listing_mode)?;
	let latest_run = latest_stale_active_run(&runs);
	let run_lease = runs.iter().any(ProjectRunStatus::run_lease);
	if run_lease {
		blockers.push(String::from("run_lease_present"));
	} else {
		evidence.push(String::from("run_lease_missing"));
	}
	let mapping = match stale_active_worktree_mapping_for_keys(state_store, &issue_keys) {
		Ok(mapping) => mapping,
		Err(error) => {
			blockers.push(String::from("worktree_mapping_ambiguous"));
			evidence.push(format!("worktree_mapping_error:{}", error));

			None
		},
	};
	let worktree_path = mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| worktree_root.join(&issue.identifier));
	let marker = match state::read_run_activity_marker_snapshot(&worktree_path) {
		Ok(marker) => marker,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			None
		},
	};
	let marker_liveness = stale_active_optional_marker_process_liveness(marker.as_ref());
	inspect_stale_active_run_evidence(&runs, marker_liveness, &mut evidence, &mut blockers);
	let worktree_state = inspect_stale_active_worktree(
		&worktree_path,
		mapping.as_ref(),
		marker.as_ref(),
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);
	let control_channel = inspect_stale_active_control_channel(
		latest_run,
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	inspect_stale_active_private_evidence(
		project_id,
		state_store,
		&issue_keys,
		latest_run,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_stale_active_review_lineage(
		project_id,
		state_store,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;
	apply_stale_active_release_reentries(
		StaleActiveReleaseReentryInput {
			run: latest_run,
			run_lease,
			active_shared_claim,
			labels: &labels,
			issue: &issue,
			tracker_policy: workflow.frontmatter().tracker(),
			worktree_state: &worktree_state,
			control_channel: &control_channel,
		},
		&mut evidence,
		&mut blockers,
	);

	let (classification, reason, next_action) =
		stale_active_diagnostic_outcome(&issue.identifier, &evidence, &blockers);

	Ok(StaleActiveDiagnostic {
		project_id: project_id.to_owned(),
		issue_id: issue.id,
		issue_identifier: issue.identifier,
		issue_state: issue.state.name,
		classification,
		reason,
		queue_label_present: labels.queue_label_present,
		active_label_present: labels.active_label_present,
		needs_attention_label_present: labels.needs_attention_label_present,
		latest_run_id: latest_run.map(|run| run.run_id().to_owned()),
		latest_attempt_number: latest_run.map(ProjectRunStatus::attempt_number),
		latest_attempt_status: latest_run.map(|run| run.status().to_owned()),
		run_lease,
		active_shared_claim,
		control_channel,
		worktree_path: Some(worktree_path.to_string_lossy().to_string()),
		worktree_state,
		evidence: super::sorted_unique(evidence),
		blockers: super::sorted_unique(blockers),
		next_action,
	})
}

fn stale_active_diagnostic_outcome(
	issue_identifier: &str,
	evidence: &[String],
	blockers: &[String],
) -> (String, String, String) {
	if blockers.is_empty() {
		if evidence_contains(evidence, "stale_active_startable_state_restore_pending") {
			(
				String::from(STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION),
				String::from(
					"queued_issue_needs_startable_state_restore_after_stale_active_release",
				),
				format!(
					"Run `decodex recover stale-active release {issue_identifier} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				),
			)
		} else {
			(
				String::from(STALE_ACTIVE_CLASSIFICATION),
				String::from(
					"tracker_issue_has_stale_active_label_without_live_or_retained_progress",
				),
				format!(
					"Run `decodex recover stale-active release {issue_identifier} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				),
			)
		}
	} else {
		(
			String::from(STALE_ACTIVE_BLOCKED_CLASSIFICATION),
			String::from("safety_check_blocked"),
			String::from(
				"Preserve the lane and inspect the listed blockers before using a recovery command.",
			),
		)
	}
}
