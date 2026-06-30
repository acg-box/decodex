use std::path::Path;

use crate::{
	commit_message,
	prelude::{Result, eyre},
};

use super::ManualAuthority;

pub(super) fn resolve_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	if manual_authority {
		return Ok(ManualAuthority::Manual);
	}

	if let Some(explicit) = explicit {
		return Ok(ManualAuthority::Issue(commit_message::normalize_issue_identifier(
			"authority",
			explicit,
		)?));
	}
	if let Some(inferred) = infer_issue_identifier_from_worktree_root(worktree_root) {
		return Ok(ManualAuthority::Issue(inferred));
	}

	if config_path.is_some() {
		eyre::bail!(
			"Failed to infer the issue authority from worktree `{}`. Pass `--authority <ISSUE>` or `--manual-authority`.",
			worktree_root.display()
		);
	}

	eyre::bail!(
		"`--authority <ISSUE>` or `--manual-authority` is required outside an issue worktree."
	)
}

pub(super) fn resolve_land_authority(
	config_path: Option<&Path>,
	explicit: Option<&str>,
	manual_authority: bool,
	worktree_root: &Path,
) -> Result<ManualAuthority> {
	if manual_authority {
		return Ok(ManualAuthority::Manual);
	}

	let inferred = infer_issue_identifier_from_worktree_root(worktree_root);

	if let Some(explicit) = explicit {
		let explicit = commit_message::normalize_issue_identifier("authority", explicit)?;

		if let Some(inferred) = inferred {
			if !explicit.eq_ignore_ascii_case(&inferred) {
				eyre::bail!(
					"`decodex land` authority `{explicit}` does not match the current lane issue `{inferred}`."
				);
			}

			return Ok(ManualAuthority::Issue(inferred));
		}

		return Ok(ManualAuthority::Issue(explicit));
	}
	if let Some(inferred) = inferred {
		return Ok(ManualAuthority::Issue(inferred));
	}

	if config_path.is_some() {
		eyre::bail!(
			"Failed to infer the lane issue from worktree `{}`. Pass `--authority <ISSUE>` or `--manual-authority`.",
			worktree_root.display()
		);
	}

	eyre::bail!(
		"`--authority <ISSUE>` or `--manual-authority` is required outside an issue worktree."
	)
}

pub(super) fn infer_issue_identifier_from_worktree_root(worktree_root: &Path) -> Option<String> {
	let basename = worktree_root.file_name()?.to_str()?;

	looks_like_issue_identifier(basename).then(|| basename.to_owned())
}

pub(super) fn looks_like_issue_identifier(value: &str) -> bool {
	commit_message::looks_like_issue_identifier(value)
}
