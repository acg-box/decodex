use std::path::Path;

use crate::{
	prelude::Result,
	recovery::{
		self, STALE_ACTIVE_BLOCKED_CLASSIFICATION, STALE_ACTIVE_CLASSIFICATION,
		STALE_ACTIVE_STATE_RESTORE_CLASSIFICATION,
		context::RecoveryRuntimeMutationPolicy,
		process_liveness::{self, StaleActiveProcessLiveness},
		reports::StaleActiveDiagnostic,
		stale_active_authority, stale_active_guidance,
		stale_active_labels::{self, StaleActiveLabelSnapshot},
		stale_active_reentry::{self, StaleActiveReleaseReentryInput},
		stale_active_runtime, stale_active_worktree,
	},
	state::{self, ProjectRunStatus, RunActivityMarker, StateStore, WorktreeMapping},
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

struct StaleActiveDiagnosticParts<'a> {
	project_id: &'a str,
	issue: TrackerIssue,
	labels: StaleActiveLabelSnapshot,
	latest_run: Option<&'a ProjectRunStatus>,
	run_lease: bool,
	active_shared_claim: bool,
	control_channel: String,
	worktree_path: &'a Path,
	worktree_state: String,
	evidence: Vec<String>,
	blockers: Vec<String>,
}

struct StaleActiveDeadOwnershipInput<'a> {
	project_id: &'a str,
	state_store: &'a StateStore,
	issue_keys: &'a [String],
	marker: Option<&'a RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	latest_run: Option<&'a ProjectRunStatus>,
	run_lease: bool,
	active_shared_claim: bool,
}

struct StaleActiveReleaseReentryInspection<'a> {
	latest_run: Option<&'a ProjectRunStatus>,
	run_lease: bool,
	active_shared_claim: bool,
	labels: &'a StaleActiveLabelSnapshot,
	issue: &'a TrackerIssue,
	workflow: &'a WorkflowDocument,
	worktree_state: &'a str,
	control_channel: &'a str,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct StaleActiveDeadLocalClaims {
	matching_claim_count: usize,
	incompatible_claim_present: bool,
}

pub(super) fn inspect_stale_active_issue<T>(
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
	let issue_keys = stale_active_labels::stale_active_tracker_issue_keys(&issue);
	let labels = stale_active_labels::inspect_stale_active_labels(
		project_id,
		workflow,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;
	let active_shared_claim = stale_active_labels::inspect_stale_active_shared_claim(
		project_id,
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	);
	let runs = stale_active_runtime::stale_active_runs(
		project_id,
		state_store,
		&issue_keys,
		listing_mode,
	)?;
	let latest_run = stale_active_runtime::latest_stale_active_run(&runs);
	let run_lease = runs.iter().any(ProjectRunStatus::run_lease);

	record_stale_active_run_lease_evidence(run_lease, &mut evidence, &mut blockers);

	let mapping =
		read_stale_active_worktree_mapping(state_store, &issue_keys, &mut evidence, &mut blockers);
	let worktree_path = mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| worktree_root.join(&issue.identifier));
	let marker = read_stale_active_activity_marker(&worktree_path, &mut evidence, &mut blockers);
	let marker_liveness =
		process_liveness::stale_active_optional_marker_process_liveness(marker.as_ref());

	record_recoverable_dead_leased_ownership(
		StaleActiveDeadOwnershipInput {
			project_id,
			state_store,
			issue_keys: &issue_keys,
			marker: marker.as_ref(),
			marker_liveness,
			latest_run,
			run_lease,
			active_shared_claim,
		},
		&mut evidence,
		&mut blockers,
	);

	stale_active_runtime::inspect_stale_active_run_evidence(
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	let worktree_state = stale_active_worktree::inspect_stale_active_worktree(
		&worktree_path,
		mapping.as_ref(),
		marker.as_ref(),
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);
	let control_channel = stale_active_runtime::inspect_stale_active_control_channel(
		latest_run,
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	inspect_stale_active_authority_evidence(
		project_id,
		state_store,
		tracker,
		&issue,
		issue_keys.as_slice(),
		latest_run,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	)?;
	apply_stale_active_release_reentry(
		StaleActiveReleaseReentryInspection {
			latest_run,
			run_lease,
			active_shared_claim,
			labels: &labels,
			issue: &issue,
			workflow,
			worktree_state: &worktree_state,
			control_channel: &control_channel,
		},
		&mut evidence,
		&mut blockers,
	);

	Ok(stale_active_diagnostic_from_parts(StaleActiveDiagnosticParts {
		project_id,
		issue,
		labels,
		latest_run,
		run_lease,
		active_shared_claim,
		control_channel,
		worktree_path: &worktree_path,
		worktree_state,
		evidence,
		blockers,
	}))
}

fn stale_active_diagnostic_from_parts(
	parts: StaleActiveDiagnosticParts<'_>,
) -> StaleActiveDiagnostic {
	let (classification, reason, next_action) =
		stale_active_diagnostic_outcome(&parts.issue.identifier, &parts.evidence, &parts.blockers);

	StaleActiveDiagnostic {
		project_id: parts.project_id.to_owned(),
		issue_id: parts.issue.id,
		issue_identifier: parts.issue.identifier,
		issue_state: parts.issue.state.name,
		classification,
		reason,
		queue_label_present: parts.labels.queue_label_present,
		active_label_present: parts.labels.active_label_present,
		needs_attention_label_present: parts.labels.needs_attention_label_present,
		latest_run_id: parts.latest_run.map(|run| run.run_id().to_owned()),
		latest_attempt_number: parts.latest_run.map(ProjectRunStatus::attempt_number),
		latest_attempt_status: parts.latest_run.map(|run| run.status().to_owned()),
		run_lease: parts.run_lease,
		active_shared_claim: parts.active_shared_claim,
		control_channel: parts.control_channel,
		worktree_path: Some(parts.worktree_path.to_string_lossy().to_string()),
		worktree_state: parts.worktree_state,
		evidence: recovery::sorted_unique(parts.evidence),
		blockers: recovery::sorted_unique(parts.blockers),
		next_action,
	}
}

#[allow(clippy::too_many_arguments)]
fn inspect_stale_active_authority_evidence<T>(
	project_id: &str,
	state_store: &StateStore,
	tracker: &T,
	issue: &TrackerIssue,
	issue_keys: &[String],
	latest_run: Option<&ProjectRunStatus>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	stale_active_authority::inspect_stale_active_private_evidence(
		project_id,
		state_store,
		issue_keys,
		latest_run,
		marker_liveness,
		evidence,
		blockers,
	)?;

	stale_active_authority::inspect_stale_active_review_lineage(
		project_id,
		state_store,
		tracker,
		issue,
		evidence,
		blockers,
	)
}

fn apply_stale_active_release_reentry(
	inspection: StaleActiveReleaseReentryInspection<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	stale_active_reentry::apply_stale_active_release_reentries(
		StaleActiveReleaseReentryInput {
			run: inspection.latest_run,
			run_lease: inspection.run_lease,
			active_shared_claim: inspection.active_shared_claim,
			labels: inspection.labels,
			issue: inspection.issue,
			tracker_policy: inspection.workflow.frontmatter().tracker(),
			worktree_state: inspection.worktree_state,
			control_channel: inspection.control_channel,
		},
		evidence,
		blockers,
	);
}

fn record_stale_active_run_lease_evidence(
	run_lease: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if run_lease {
		blockers.push(String::from("run_lease_present"));
	} else {
		evidence.push(String::from("run_lease_missing"));
	}
}

fn read_stale_active_worktree_mapping(
	state_store: &StateStore,
	issue_keys: &[String],
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<WorktreeMapping> {
	match stale_active_worktree::stale_active_worktree_mapping_for_keys(state_store, issue_keys) {
		Ok(mapping) => mapping,
		Err(error) => {
			blockers.push(String::from("worktree_mapping_ambiguous"));
			evidence.push(format!("worktree_mapping_error:{}", error));

			None
		},
	}
}

fn read_stale_active_activity_marker(
	worktree_path: &Path,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Option<RunActivityMarker> {
	match state::read_run_activity_marker_snapshot(worktree_path) {
		Ok(marker) => marker,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			None
		},
	}
}

fn record_recoverable_dead_leased_ownership(
	input: StaleActiveDeadOwnershipInput<'_>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let Some(latest_run) = input.latest_run else {
		return;
	};

	if !(input.run_lease
		&& stale_active_dead_marker_matches_run(input.marker, input.marker_liveness, latest_run))
	{
		return;
	}

	let Ok(local_claims) = stale_active_dead_local_claims_for_run(
		input.project_id,
		input.state_store,
		input.issue_keys,
		latest_run,
	) else {
		blockers.push(String::from("active_shared_claim_unknown"));
		evidence.push(String::from("active_shared_claim_error:dead_local_claim_inspection_failed"));

		return;
	};

	if local_claims.matching_claim_count == 0 {
		return;
	}
	if local_claims.incompatible_claim_present {
		evidence.push(String::from("stale_active_claim_identity_mismatch_present"));

		return;
	}

	blockers.retain(|blocker| blocker != "run_lease_present");
	evidence.push(String::from("stale_run_lease_present"));

	if input.active_shared_claim {
		blockers.retain(|blocker| blocker != "active_shared_claim_present");
		evidence.push(String::from("stale_active_shared_claim_present"));
	}
}

fn stale_active_dead_marker_matches_run(
	marker: Option<&RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	run: &ProjectRunStatus,
) -> bool {
	marker_liveness == StaleActiveProcessLiveness::NotAlive
		&& marker.is_some_and(|marker| {
			marker.run_id() == run.run_id() && marker.attempt_number() == run.attempt_number()
		})
}

fn stale_active_dead_local_claims_for_run(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	run: &ProjectRunStatus,
) -> Result<StaleActiveDeadLocalClaims> {
	let mut claims = StaleActiveDeadLocalClaims::default();

	for issue_key in issue_keys {
		let local_claim_matches =
			state_store.lease_for_issue(issue_key)?.as_ref().is_some_and(|lease| {
				lease.project_id() == project_id && lease.run_id() == run.run_id()
			});
		let active_claim =
			state_store.issue_has_active_shared_claim_read_only(project_id, issue_key)?;
		let external_claim =
			state_store.issue_has_external_shared_claim_read_only(project_id, issue_key)?;

		if local_claim_matches {
			claims.matching_claim_count += 1;
		}
		if external_claim || (active_claim && !local_claim_matches) {
			claims.incompatible_claim_present = true;
		}
	}

	Ok(claims)
}

fn stale_active_diagnostic_outcome(
	issue_identifier: &str,
	evidence: &[String],
	blockers: &[String],
) -> (String, String, String) {
	if blockers.is_empty() {
		if stale_active_reentry::evidence_contains(
			evidence,
			"stale_active_startable_state_restore_pending",
		) {
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
			stale_active_guidance::blocked_stale_active_next_action(
				issue_identifier,
				blockers,
				evidence,
			),
		)
	}
}
