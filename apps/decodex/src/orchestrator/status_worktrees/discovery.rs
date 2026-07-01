use std::collections::HashSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, OperatorSnapshotWarningDetail, OperatorWorktreeStatus, status_run_projection,
		status_worktrees::{cleanup_debt, provenance},
	},
	prelude::Result,
	state::{StateStore, WORKTREE_PROVENANCE_FILESYSTEM_SCAN},
};

pub(crate) fn operator_status_worktrees(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<(Vec<OperatorWorktreeStatus>, Vec<String>, Vec<OperatorSnapshotWarningDetail>)> {
	let skipped_terminal_local_issue_ids = stale_terminal_local_issue_ids(project, state_store)?;
	let mut worktrees = Vec::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if skipped_terminal_local_issue_ids.contains(mapping.issue_id()) {
			continue;
		}

		worktrees.push(OperatorWorktreeStatus {
			project_id: project.service_id().to_owned(),
			issue_id: mapping.issue_id().to_owned(),
			issue_identifier: status_run_projection::issue_identifier_in_text(
				mapping.branch_name(),
			)
			.or_else(|| {
				status_run_projection::issue_identifier_in_text(
					&mapping.worktree_path().display().to_string(),
				)
			}),
			issue_state: None,
			branch_name: mapping.branch_name().to_owned(),
			worktree_path: orchestrator::relative_worktree_path_for_path(
				project,
				mapping.worktree_path(),
			),
			ownership: String::from("cleanup_only"),
			ownership_reason: String::from(
				"No current lane, queued recovery, or post-review lane currently owns this worktree.",
			),
			provenance: provenance::operator_worktree_provenance_from_mapping(&mapping),
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

	for issue_identifier in orchestrator::recoverable_worktree_identifiers(project.worktree_root())?
	{
		let worktree_path = project.worktree_root().join(&issue_identifier);
		let relative_path = orchestrator::relative_worktree_path_for_path(project, &worktree_path);

		if !seen_paths.insert(relative_path.clone()) {
			continue;
		}

		let branch_name = orchestrator::worktree_checkout_branch_name(&worktree_path)
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
			provenance: provenance::operator_worktree_provenance(
				WORKTREE_PROVENANCE_FILESYSTEM_SCAN,
				None,
				None,
			),
			recovery_next_action: None,
			hygiene: None,
		});
	}

	cleanup_debt::append_merged_worktree_cleanup_debts(
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

pub(crate) fn active_shared_issue_ids(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<HashSet<String>> {
	Ok(state_store
		.list_active_shared_leases(project.service_id())?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect())
}

pub(crate) fn stale_terminal_local_issue_ids(
	project: &ServiceConfig,
	state_store: &StateStore,
) -> Result<HashSet<String>> {
	let active_issue_ids = active_shared_issue_ids(project, state_store)?;
	let mut issue_ids = HashSet::new();

	for mapping in state_store.list_worktrees(project.service_id())? {
		if orchestrator::worktree_mapping_is_stale_terminal_local_residue(
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
