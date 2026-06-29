//! Worktree ownership and hygiene projection for operator status snapshots.

use std::collections::HashSet;

use color_eyre::Report;

use crate::{
	config::ServiceConfig,
	state::{
		StateStore, WORKTREE_PROVENANCE_FILESYSTEM_SCAN, WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN,
		WORKTREE_PROVENANCE_LEGACY_UNKNOWN, WorktreeMapping,
	},
	worktree::{self, MergedWorktreeCleanupDebt},
};

use super::{
	OperatorHistoryLaneStatus, OperatorPostReviewLaneStatus, OperatorRunStatus,
	OperatorSnapshotWarningDetail, OperatorStatusSnapshot, OperatorWorktreeHygieneStatus,
	OperatorWorktreeProvenanceStatus, OperatorWorktreeStatus,
	QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT, WorktreeOwnership, history_lane_group_key,
	history_ledger_outcome_requires_attention, issue_identifier_in_text,
	operator_issue_attention_key, recoverable_worktree_identifiers,
	relative_worktree_path_for_path, worktree_checkout_branch_name,
	worktree_mapping_is_stale_terminal_local_residue,
};

pub(crate) fn ensure_project_has_no_merged_worktree_cleanup_debt(
	project: &ServiceConfig,
) -> crate::prelude::Result<()> {
	let debts = project_merged_worktree_cleanup_debts(project)?;

	if debts.is_empty() {
		return Ok(());
	}

	crate::prelude::eyre::bail!(
		"Post-land worktree cleanup is pending for project `{}`; remove or salvage merged linked worktrees before continuing automation: {}",
		project.service_id(),
		format_merged_worktree_cleanup_debts(&debts)
	);
}

pub(super) fn refresh_worktree_ownership(
	snapshot: &mut OperatorStatusSnapshot,
	completed_state: Option<&str>,
) {
	let ownership = snapshot
		.worktrees
		.iter()
		.map(|worktree| worktree_ownership(worktree, snapshot, completed_state))
		.collect::<Vec<_>>();

	for (worktree, ownership) in snapshot.worktrees.iter_mut().zip(ownership) {
		worktree.ownership = ownership.kind.to_owned();
		worktree.ownership_reason = ownership.reason;
		worktree.recovery_next_action = ownership.next_action;
		worktree.provenance.audit_required = ownership.audit_required;
	}
}

pub(super) fn operator_status_worktrees(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<(
	Vec<OperatorWorktreeStatus>,
	Vec<String>,
	Vec<OperatorSnapshotWarningDetail>,
)> {
	let skipped_terminal_local_issue_ids = stale_terminal_local_issue_ids(project, state_store)?;
	let mut worktrees = Vec::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if skipped_terminal_local_issue_ids.contains(mapping.issue_id()) {
			continue;
		}

		worktrees.push(OperatorWorktreeStatus {
			project_id: project.service_id().to_owned(),
			issue_id: mapping.issue_id().to_owned(),
			issue_identifier: issue_identifier_in_text(mapping.branch_name()).or_else(|| {
				issue_identifier_in_text(&mapping.worktree_path().display().to_string())
			}),
			issue_state: None,
			branch_name: mapping.branch_name().to_owned(),
			worktree_path: relative_worktree_path_for_path(project, mapping.worktree_path()),
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No current lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			provenance: operator_worktree_provenance_from_mapping(&mapping),
			recovery_next_action: None,
			hygiene: None,
		});
	}

	let mut seen_paths =
		worktrees.iter().map(|worktree| worktree.worktree_path.clone()).collect::<HashSet<_>>();
	let mut warnings = Vec::new();
	let mut warning_details = Vec::new();
	let mut skipped_terminal_local_issue_ids =
		skipped_terminal_local_issue_ids.into_iter().collect::<Vec<_>>();

	skipped_terminal_local_issue_ids.sort();

	append_stale_terminal_local_mapping_warning(
		project,
		&skipped_terminal_local_issue_ids,
		&mut warnings,
		&mut warning_details,
	);

	for issue_identifier in recoverable_worktree_identifiers(project.worktree_root())? {
		let worktree_path = project.worktree_root().join(&issue_identifier);
		let relative_path = relative_worktree_path_for_path(project, &worktree_path);

		if !seen_paths.insert(relative_path.clone()) {
			continue;
		}

		let branch_name = worktree_checkout_branch_name(&worktree_path)
			.ok()
			.flatten()
			.unwrap_or_else(|| issue_identifier.clone());

		worktrees.push(OperatorWorktreeStatus {
			project_id: project.service_id().to_owned(),
			issue_identifier: Some(issue_identifier.clone()),
			issue_id: issue_identifier,
			issue_state: None,
			branch_name,
			worktree_path: relative_path,
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No current lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			provenance: operator_worktree_provenance(
				WORKTREE_PROVENANCE_FILESYSTEM_SCAN,
				None,
				None,
			),
			recovery_next_action: None,
			hygiene: None,
		});
	}

	append_merged_worktree_cleanup_debts(
		project,
		&mut worktrees,
		&mut seen_paths,
		&mut warnings,
		&mut warning_details,
	);

	worktrees.sort_by(|left, right| {
		left.issue_id
			.cmp(&right.issue_id)
			.then_with(|| left.branch_name.cmp(&right.branch_name))
			.then_with(|| left.worktree_path.cmp(&right.worktree_path))
	});

	Ok((worktrees, warnings, warning_details))
}

pub(super) fn active_shared_issue_ids(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<HashSet<String>> {
	Ok(state_store
		.list_active_shared_leases(project.service_id())?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect())
}

pub(super) fn stale_terminal_local_issue_ids(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> crate::prelude::Result<HashSet<String>> {
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
	let mut issue_ids = HashSet::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if worktree_mapping_is_stale_terminal_local_residue(
			project,
			state_store,
			&mapping,
			&active_issue_ids,
		)? {
			issue_ids.insert(mapping.issue_id().to_owned());
		}
	}

	Ok(issue_ids)
}

fn worktree_ownership(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
	completed_state: Option<&str>,
) -> WorktreeOwnership {
	let post_review_owner = worktree_post_review_owner(worktree, snapshot);

	if let Some(run) = worktree_current_lane_owner(worktree, snapshot) {
		if run.ownership_state == "orphaned_live_thread"
			&& let Some(lane) = post_review_owner
		{
			return post_review_worktree_ownership(lane);
		}

		return match run.ownership_state.as_str() {
			"leased_run" => WorktreeOwnership {
				kind: "current_lane",
				reason: format!("Current lane `{}` owns this worktree.", run.run_id),
				next_action: None,
				audit_required: false,
			},
			"retained_attention" => WorktreeOwnership {
				kind: "retained_attention",
				reason: format!(
					"Lane `{}` requires operator attention before it can own this worktree.",
					run.run_id
				),
				next_action: Some(run.lane_control_next_action.clone()),
				audit_required: true,
			},
			"orphaned_live_thread" => WorktreeOwnership {
				kind: "orphaned_live_thread",
				reason: format!(
					"Lane `{}` has live evidence but no active Decodex lease.",
					run.run_id
				),
				next_action: Some(run.lane_control_next_action.clone()),
				audit_required: true,
			},
			"terminalizing" => WorktreeOwnership {
				kind: "terminalizing_lane",
				reason: format!(
					"Lane `{}` is inside terminalization and no longer counts as running.",
					run.run_id
				),
				next_action: Some(run.lane_control_next_action.clone()),
				audit_required: true,
			},
			"continuation_pending" => WorktreeOwnership {
				kind: "continuation_pending",
				reason: format!(
					"Lane `{}` is waiting for scheduled continuation re-entry.",
					run.run_id
				),
				next_action: Some(run.lane_control_next_action.clone()),
				audit_required: false,
			},
			_ => WorktreeOwnership {
				kind: "orphaned_local_worktree",
				reason: format!("Lane `{}` is not an active owner for this worktree.", run.run_id),
				next_action: Some(run.lane_control_next_action.clone()),
				audit_required: true,
			},
		};
	}
	if let Some(lane) = post_review_owner {
		return post_review_worktree_ownership(lane);
	}
	if let Some(lane) = worktree_history_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "retained_attention",
			reason: format!(
				"Run Ledger owns this worktree through terminal `{}` outcome.",
				lane.ledger_outcome.final_outcome
			),
			next_action: Some(lane.ledger_outcome.needs_attention_reason.clone().unwrap_or_else(
				|| {
					String::from(
						"inspect the retained worktree diff and resolve the terminal attention outcome manually",
					)
				},
			)),
			audit_required: false,
		};
	}

	if worktree_has_queued_attention_owner(worktree, snapshot) {
		return WorktreeOwnership {
			kind: "queued_attention",
			reason: String::from(
				"Intake Queue owns this worktree because the issue needs operator attention.",
			),
			next_action: None,
			audit_required: false,
		};
	}

	if let Some(hygiene) = &worktree.hygiene {
		return WorktreeOwnership {
			kind: "post_land_cleanup",
			reason: hygiene.reason.clone(),
			next_action: Some(String::from(
				"inspect the merged worktree, preserve or discard local changes intentionally, then remove the linked worktree",
			)),
			audit_required: false,
		};
	}

	let audit_required = worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN;

	WorktreeOwnership {
		kind: "cleanup_only",
		reason: worktree_cleanup_only_reason(worktree, completed_state),
		next_action: audit_required.then(|| legacy_cleanup_next_action(worktree)),
		audit_required,
	}
}

fn post_review_worktree_ownership(lane: &OperatorPostReviewLaneStatus) -> WorktreeOwnership {
	WorktreeOwnership {
		kind: "post_review_lane",
		reason: format!("Review & Landing owns this worktree as `{}`.", lane.classification),
		next_action: None,
		audit_required: false,
	}
}

fn worktree_current_lane_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorRunStatus> {
	snapshot.current_lanes.iter().chain(snapshot.recent_runs.iter()).find(|run| {
		matches!(
			run.ownership_state.as_str(),
			"leased_run"
				| "retained_attention"
				| "orphaned_live_thread"
				| "terminalizing"
				| "continuation_pending"
		) && (run.worktree_path.as_deref() == Some(worktree.worktree_path.as_str())
			|| run.branch_name.as_deref() == Some(worktree.branch_name.as_str())
			|| run.issue_id == worktree.issue_id)
	})
}

fn worktree_post_review_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorPostReviewLaneStatus> {
	snapshot.post_review_lanes.iter().find(|lane| {
		lane.worktree_path == worktree.worktree_path
			|| lane.branch_name == worktree.branch_name
			|| lane.issue_id == worktree.issue_id
			|| lane.issue_identifier == worktree.issue_id
			|| worktree.issue_identifier.as_deref() == Some(lane.issue_identifier.as_str())
	})
}

fn worktree_has_queued_attention_owner(
	worktree: &OperatorWorktreeStatus,
	snapshot: &OperatorStatusSnapshot,
) -> bool {
	snapshot.queued_candidates.iter().any(|candidate| {
		matches!(
			candidate.reason.as_str(),
			"issue_needs_attention" | QUEUE_REASON_LINEAR_ACTIVE_LABEL_PRESENT
		) && (candidate.attention.as_ref().and_then(|attention| attention.worktree_path.as_deref())
			== Some(worktree.worktree_path.as_str())
			|| candidate.issue_id == worktree.issue_id
			|| candidate.issue_identifier == worktree.issue_id
			|| worktree.issue_identifier.as_deref() == Some(candidate.issue_identifier.as_str()))
	})
}

fn worktree_history_attention_owner<'a>(
	worktree: &OperatorWorktreeStatus,
	snapshot: &'a OperatorStatusSnapshot,
) -> Option<&'a OperatorHistoryLaneStatus> {
	let worktree_issue_key =
		operator_issue_attention_key(&worktree.issue_id, worktree.issue_identifier.as_deref());

	snapshot.history_lanes.iter().find(|lane| {
		history_ledger_outcome_requires_attention(&lane.ledger_outcome)
			&& (history_lane_group_key(lane) == worktree_issue_key
				|| lane.latest_run.worktree_path.as_deref()
					== Some(worktree.worktree_path.as_str())
				|| lane.latest_run.branch_name.as_deref() == Some(worktree.branch_name.as_str()))
	})
}

fn worktree_cleanup_only_reason(
	worktree: &OperatorWorktreeStatus,
	completed_state: Option<&str>,
) -> String {
	if worktree.provenance.source == WORKTREE_PROVENANCE_LEGACY_UNKNOWN {
		return String::from(
			"Legacy worktree mapping has no durable runtime provenance; no active, queued, or post-review lane owns it, so Decodex cannot automatically prove PR or closeout lineage.",
		);
	}

	if let (Some(issue_state), Some(completed_state)) =
		(worktree.issue_state.as_deref(), completed_state)
		&& issue_state == completed_state
	{
		return format!(
			"Issue is {completed_state}; no active or post-review lane owns this worktree, so it is local cleanup only."
		);
	}

	String::from(
		"No current lane, queued recovery, or post-review lane owns this worktree; local cleanup only.",
	)
}

fn legacy_cleanup_next_action(worktree: &OperatorWorktreeStatus) -> String {
	let issue = worktree.issue_identifier.as_deref().unwrap_or(&worktree.issue_id);

	format!(
		"verify tracker/PR terminal state and clean git status for `{}`, then run `decodex recover legacy-closeout {issue} --pr <MERGED_PR> --dry-run`; rerun with `--manual-authority` before removing this worktree",
		worktree.worktree_path
	)
}

fn append_stale_terminal_local_mapping_warning(
	project: &ServiceConfig,
	issue_ids: &[String],
	warnings: &mut Vec<String>,
	warning_details: &mut Vec<OperatorSnapshotWarningDetail>,
) {
	if issue_ids.is_empty() {
		return;
	}

	let warning = String::from("stale_terminal_local_worktree_mapping_ignored");

	warnings.push(warning.clone());
	warning_details.push(OperatorSnapshotWarningDetail {
		warning,
		project_id: Some(project.service_id().to_owned()),
		repo_root: None,
		reason: format!(
			"ignored {} terminal unleased runtime-recorded worktree mapping(s) with identifier-style issue ids and missing paths: {}",
			issue_ids.len(),
			issue_ids.join(", ")
		),
		next_action: Some(String::from(
			"no recovery worktree action is required; project reconciliation clears this stale local mapping before tracker refresh",
		)),
	});
}

fn append_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
	worktrees: &mut Vec<OperatorWorktreeStatus>,
	seen_paths: &mut HashSet<String>,
	warnings: &mut Vec<String>,
	warning_details: &mut Vec<OperatorSnapshotWarningDetail>,
) {
	let debts = match project_merged_worktree_cleanup_debts(project) {
		Ok(debts) => debts,
		Err(error) => {
			tracing::warn!(
				project_id = project.service_id(),
				error = %error,
				"Skipped merged worktree cleanup debt scan while publishing an operator snapshot."
			);

			warnings.push(String::from("worktree_hygiene_unavailable"));
			warning_details.push(worktree_hygiene_unavailable_warning_detail(project, &error));

			return;
		},
	};

	if debts.is_empty() {
		return;
	}

	let mut surfaced_cleanup_debt = false;
	let mut surfaced_dirty_cleanup_debt = false;

	for debt in debts {
		let relative_path = relative_worktree_path_for_path(project, &debt.path);
		let is_dirty = debt.cleanliness.is_dirty();
		let debt_status = operator_worktree_status_from_cleanup_debt(
			project.service_id(),
			debt,
			relative_path.clone(),
		);

		if !seen_paths.insert(relative_path.clone()) {
			if let Some(existing) =
				worktrees.iter_mut().find(|worktree| worktree.worktree_path == relative_path)
			{
				existing.hygiene = debt_status.hygiene;
			}

			continue;
		}

		surfaced_cleanup_debt = true;
		surfaced_dirty_cleanup_debt |= is_dirty;

		worktrees.push(debt_status);
	}

	if surfaced_cleanup_debt {
		warnings.push(String::from("merged_worktree_cleanup_pending"));
	}
	if surfaced_dirty_cleanup_debt {
		warnings.push(String::from("merged_dirty_worktree"));
	}
}

fn worktree_hygiene_unavailable_warning_detail(
	project: &ServiceConfig,
	error: &Report,
) -> OperatorSnapshotWarningDetail {
	OperatorSnapshotWarningDetail {
		warning: String::from("worktree_hygiene_unavailable"),
		project_id: Some(project.service_id().to_owned()),
		repo_root: Some(project.repo_root().display().to_string()),
		reason: format!("Worktree hygiene scan failed: {error}"),
		next_action: Some(String::from(
			"Remove the stale project registration or restore the Git checkout before running automation.",
		)),
	}
}

fn operator_worktree_status_from_cleanup_debt(
	project_id: &str,
	debt: MergedWorktreeCleanupDebt,
	relative_path: String,
) -> OperatorWorktreeStatus {
	let dirty = debt.cleanliness.is_dirty();
	let classification =
		if dirty { "merged_dirty_worktree" } else { "merged_worktree_cleanup_pending" };
	let default_branch = debt.default_branch.clone();
	let reason = format!(
		"Branch `{}` is already merged into `{}` but linked worktree `{}` still exists{}; remove or salvage it before continuing automation.",
		debt.branch_name,
		default_branch,
		relative_path,
		if dirty { " with local changes" } else { "" },
	);
	let branch_name = debt.branch_name;

	OperatorWorktreeStatus {
		project_id: project_id.to_owned(),
		issue_id: branch_name.clone(),
		issue_identifier: issue_identifier_in_text(&branch_name)
			.or_else(|| issue_identifier_in_text(&relative_path)),
		issue_state: None,
		branch_name,
		worktree_path: relative_path,
		ownership: String::from("post_land_cleanup"),
		ownership_reason: reason.clone(),
		provenance: operator_worktree_provenance(WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN, None, None),
		recovery_next_action: Some(String::from(
			"inspect the merged worktree, preserve or discard local changes intentionally, then remove the linked worktree",
		)),
		hygiene: Some(OperatorWorktreeHygieneStatus {
			classification: String::from(classification),
			default_branch,
			dirty,
			reason,
		}),
	}
}

fn operator_worktree_provenance_from_mapping(
	mapping: &WorktreeMapping,
) -> OperatorWorktreeProvenanceStatus {
	operator_worktree_provenance(
		mapping.provenance().source(),
		mapping.provenance().created_at_unix(),
		mapping.provenance().updated_at_unix(),
	)
}

fn operator_worktree_provenance(
	source: &str,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
) -> OperatorWorktreeProvenanceStatus {
	OperatorWorktreeProvenanceStatus {
		source: source.to_owned(),
		created_at_unix,
		updated_at_unix,
		audit_required: false,
	}
}

fn project_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
) -> crate::prelude::Result<Vec<MergedWorktreeCleanupDebt>> {
	let Some(default_branch) = worktree::infer_default_branch_name(project.repo_root())? else {
		return Ok(Vec::new());
	};

	worktree::merged_worktree_cleanup_debts(
		project.repo_root(),
		project.worktree_root(),
		&default_branch,
	)
}

fn format_merged_worktree_cleanup_debts(debts: &[MergedWorktreeCleanupDebt]) -> String {
	debts
		.iter()
		.map(|debt| {
			format!(
				"{} on {} ({})",
				debt.path.display(),
				debt.branch_name,
				if debt.cleanliness.is_dirty() { "dirty" } else { "clean" }
			)
		})
		.collect::<Vec<_>>()
		.join(", ")
}
