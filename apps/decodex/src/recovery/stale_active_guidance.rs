//! Operator next-action guidance for blocked stale-active recovery diagnostics.

use std::collections::BTreeSet;

const PRIVATE_PROGRESS_EVIDENCE_REF_PREFIX: &str = "private_progress_evidence_ref:";
const REVIEW_AUTHORITY_BLOCKERS: &[&str] = &[
	"pr_or_review_lineage_present",
	"review_lifecycle_present",
	"review_policy_checkpoint_present",
];
const RETAINED_PROGRESS_BLOCKERS: &[&str] = &[
	"non_git_worktree_files_present",
	"private_progress_evidence_present",
	"worktree_tracked_changes_present",
	"worktree_unmerged_commits_present",
];
const LIVE_OR_UNSETTLED_BLOCKERS: &[&str] = &[
	"active_control_channel_present",
	"active_shared_claim_present",
	"activity_marker_child_agent_activity_present",
	"activity_marker_progress_present",
	"activity_marker_protocol_activity_present",
	"activity_marker_protocol_activity_summary_present",
	"activity_marker_thread_active",
	"child_agent_activity_present",
	"process_alive",
	"protocol_activity_present",
	"protocol_event_evidence_present",
	"protocol_event_marker_present",
	"run_lease_present",
];

pub(super) fn blocked_stale_active_next_action(
	issue_identifier: &str,
	blockers: &[String],
	evidence: &[String],
) -> String {
	if blockers_include_any(blockers, REVIEW_AUTHORITY_BLOCKERS) {
		return format!(
			"Preserve the lane; review or PR authority is present. Run `decodex recover review-handoff diagnose {issue_identifier} --json` and follow the review-handoff recovery path instead of stale-active release."
		);
	}
	if blockers_include_any(blockers, RETAINED_PROGRESS_BLOCKERS) {
		let evidence_command = retained_progress_evidence_command(issue_identifier, evidence);

		return format!(
			"Preserve retained progress; do not run stale-active release. Inspect local private evidence with `{}` and any private_progress_evidence_ref entries in this report, and inspect the retained worktree from this report; then resume the same lane, recover review handoff if PR lineage exists, or route manual attention before discarding any work.",
			evidence_command,
		);
	}
	if blockers_include_any(blockers, LIVE_OR_UNSETTLED_BLOCKERS) {
		return format!(
			"Preserve the lane; runtime ownership still appears live or unsettled. Re-run `decodex status --live` or `decodex lane inspect {issue_identifier}` and wait, interrupt, or resume through lane-control only after the same run identity is proven."
		);
	}
	if blockers.iter().any(|blocker| blocker.ends_with("_unknown"))
		|| blockers_include_any(
			blockers,
			&[
				"active_label_missing",
				"needs_attention_label_present",
				"process_liveness_unknown",
				"worktree_default_branch_unavailable",
				"worktree_mapping_ambiguous",
				"worktree_tracked_changes_unknown",
			],
		) {
		return format!(
			"Preserve the lane; safety evidence is missing or contradictory. Resolve the listed blockers, then rerun `decodex recover stale-active diagnose {issue_identifier} --json` before any release attempt."
		);
	}

	String::from(
		"Preserve the lane and inspect the listed blockers before using a recovery command.",
	)
}

fn retained_progress_evidence_command(issue_identifier: &str, evidence: &[String]) -> String {
	let commands = private_progress_evidence_commands(issue_identifier, evidence);

	if commands.is_empty() {
		format!("decodex evidence {issue_identifier} --json")
	} else {
		commands.join("`, `")
	}
}

fn private_progress_evidence_commands(issue_identifier: &str, evidence: &[String]) -> Vec<String> {
	let mut commands = BTreeSet::new();

	for item in evidence {
		let Some(ref_value) = item.strip_prefix(PRIVATE_PROGRESS_EVIDENCE_REF_PREFIX) else {
			continue;
		};
		let mut parts = ref_value.splitn(3, ':');
		let (Some(run_id), Some(attempt_number), Some(_event_type)) =
			(parts.next(), parts.next(), parts.next())
		else {
			continue;
		};

		commands.insert(format!(
			"decodex evidence {issue_identifier} --run-id {run_id} --attempt {attempt_number} --json"
		));
	}

	commands.into_iter().collect()
}

fn blockers_include_any(blockers: &[String], needles: &[&str]) -> bool {
	blockers.iter().any(|blocker| needles.iter().any(|needle| blocker == needle))
}
