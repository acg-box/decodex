use std::collections::HashSet;

use color_eyre::Report;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		self, OperatorSnapshotWarningDetail, OperatorWorktreeHygieneStatus, OperatorWorktreeStatus,
		status_run_projection, status_worktrees::provenance,
	},
	prelude::Result,
	state::WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN,
	worktree::{self, MergedWorktreeCleanupDebt},
};

pub(crate) fn ensure_project_has_no_merged_worktree_cleanup_debt(
	project: &ServiceConfig,
) -> Result<()> {
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

pub(in crate::orchestrator::status_worktrees) fn append_merged_worktree_cleanup_debts(
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
		let relative_path = orchestrator::relative_worktree_path_for_path(project, &debt.path);
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
		issue_identifier: status_run_projection::issue_identifier_in_text(&branch_name)
			.or_else(|| status_run_projection::issue_identifier_in_text(&relative_path)),
		issue_state: None,
		branch_name,
		worktree_path: relative_path,
		ownership: String::from("post_land_cleanup"),
		ownership_reason: reason.clone(),
		provenance: provenance::operator_worktree_provenance(
			WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN,
			None,
			None,
		),
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

fn project_merged_worktree_cleanup_debts(
	project: &ServiceConfig,
) -> Result<Vec<MergedWorktreeCleanupDebt>> {
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
