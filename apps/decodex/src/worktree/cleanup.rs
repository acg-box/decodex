use std::{
	fs,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{
	prelude::{Result, eyre},
	state,
	worktree::git,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MergedWorktreeCleanupDebt {
	pub(crate) branch_name: String,
	pub(crate) cleanliness: MergedWorktreeCleanliness,
	pub(crate) default_branch: String,
	pub(crate) path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LinkedWorktree {
	branch_name: String,
	path: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MergedWorktreeCleanliness {
	Clean,
	Dirty,
}
impl MergedWorktreeCleanliness {
	pub(crate) fn is_dirty(self) -> bool {
		self == Self::Dirty
	}
}

pub(crate) fn infer_default_branch_name(repo_root: &Path) -> Result<Option<String>> {
	if let Some(remote_head) = symbolic_ref(repo_root, "refs/remotes/origin/HEAD")?
		&& let Some(branch_name) = remote_head.strip_prefix("origin/")
		&& !branch_name.is_empty()
	{
		return Ok(Some(branch_name.to_owned()));
	}

	current_branch_name(repo_root)
}

pub(crate) fn merged_worktree_cleanup_debts(
	repo_root: &Path,
	worktree_root: &Path,
	default_branch: &str,
) -> Result<Vec<MergedWorktreeCleanupDebt>> {
	if default_branch.is_empty() || !worktree_root.exists() {
		return Ok(Vec::new());
	}

	let mut debts = Vec::new();

	for worktree in linked_worktrees(repo_root)? {
		if worktree.branch_name == default_branch
			|| linked_worktree_under_root(&worktree.path, worktree_root)?.is_none()
			|| branch_merged_into_default(repo_root, &worktree.branch_name, default_branch)?
				.is_none()
		{
			continue;
		}

		debts.push(MergedWorktreeCleanupDebt {
			branch_name: worktree.branch_name,
			cleanliness: worktree_cleanliness(&worktree.path)?,
			default_branch: default_branch.to_owned(),
			path: worktree.path,
		});
	}

	debts.sort_by(|left, right| {
		left.path.cmp(&right.path).then_with(|| left.branch_name.cmp(&right.branch_name))
	});

	Ok(debts)
}

fn linked_worktrees(repo_root: &Path) -> Result<Vec<LinkedWorktree>> {
	Ok(parse_linked_worktrees(&git::git_stdout(
		repo_root,
		["worktree", "list", "--porcelain"],
		"list linked worktrees",
	)?))
}

fn parse_linked_worktrees(output: &str) -> Vec<LinkedWorktree> {
	let mut entries = Vec::new();
	let mut current_path: Option<PathBuf> = None;
	let mut current_branch: Option<String> = None;

	for line in output.lines() {
		if line.is_empty() {
			push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

			continue;
		}

		if let Some(path) = line.strip_prefix("worktree ") {
			push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

			current_path = Some(PathBuf::from(path));

			continue;
		}
		if let Some(branch_ref) = line.strip_prefix("branch refs/heads/") {
			current_branch = Some(branch_ref.to_owned());
		}
	}

	push_linked_worktree_entry(&mut entries, &mut current_path, &mut current_branch);

	entries
}

fn push_linked_worktree_entry(
	entries: &mut Vec<LinkedWorktree>,
	path: &mut Option<PathBuf>,
	branch_name: &mut Option<String>,
) {
	if let (Some(path), Some(branch_name)) = (path.take(), branch_name.take()) {
		entries.push(LinkedWorktree { branch_name, path });
	}

	*path = None;
	*branch_name = None;
}

fn linked_worktree_under_root(path: &Path, worktree_root: &Path) -> Result<Option<()>> {
	if !path.exists() || !worktree_root.exists() {
		return Ok(None);
	}

	let canonical_path = fs::canonicalize(path)?;
	let canonical_root = fs::canonicalize(worktree_root)?;

	if canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root {
		return Ok(Some(()));
	}

	Ok(None)
}

fn branch_merged_into_default(
	repo_root: &Path,
	branch_name: &str,
	default_branch: &str,
) -> Result<Option<()>> {
	let branch_ref = format!("refs/heads/{branch_name}");
	let default_ref = format!("refs/heads/{default_branch}");

	if !git_ref_exists(repo_root, &branch_ref)? || !git_ref_exists(repo_root, &default_ref)? {
		return Ok(None);
	}
	if git_refs_point_to_same_tip(repo_root, &branch_ref, &default_ref)? {
		return Ok(None);
	}
	if branch_tip_is_on_default_first_parent(repo_root, &branch_ref, &default_ref)? {
		return Ok(None);
	}

	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", branch_ref.as_str(), default_ref.as_str()])
		.output()?;

	if output.status.success() {
		return Ok(Some(()));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to determine whether worktree branch `{branch_name}` is merged into `{default_branch}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

fn branch_tip_is_on_default_first_parent(
	repo_root: &Path,
	branch_ref: &str,
	default_ref: &str,
) -> Result<bool> {
	let branch_tip = git::git_stdout(repo_root, ["rev-parse", branch_ref], "resolve branch tip")?;
	let first_parent_history = git::git_stdout(
		repo_root,
		["rev-list", "--first-parent", default_ref],
		"list default branch first-parent history",
	)?;

	Ok(first_parent_history.lines().any(|commit| commit == branch_tip))
}

fn git_refs_point_to_same_tip(repo_root: &Path, left_ref: &str, right_ref: &str) -> Result<bool> {
	let left_tip = git::git_stdout(repo_root, ["rev-parse", left_ref], "resolve git ref tip")?;
	let right_tip = git::git_stdout(repo_root, ["rev-parse", right_ref], "resolve git ref tip")?;

	Ok(left_tip == right_tip)
}

fn git_ref_exists(repo_root: &Path, ref_name: &str) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", ref_name])
		.output()?;

	if output.status.success() {
		return Ok(true);
	}
	if output.status.code() == Some(1) {
		return Ok(false);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect git ref `{ref_name}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

fn worktree_cleanliness(worktree_path: &Path) -> Result<MergedWorktreeCleanliness> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect worktree cleanliness in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let status = String::from_utf8_lossy(&output.stdout);

	if status
		.lines()
		.filter(|line| !line.trim_end().is_empty())
		.any(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
	{
		return Ok(MergedWorktreeCleanliness::Dirty);
	}

	Ok(MergedWorktreeCleanliness::Clean)
}

fn symbolic_ref(repo_root: &Path, ref_name: &str) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["symbolic-ref", "--quiet", "--short", ref_name])
		.output()?;

	if !output.status.success() {
		return Ok(None);
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	Ok((!value.is_empty()).then_some(value))
}

fn current_branch_name(repo_root: &Path) -> Result<Option<String>> {
	let output =
		Command::new("git").arg("-C").arg(repo_root).args(["branch", "--show-current"]).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect current branch in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	Ok((!value.is_empty()).then_some(value))
}
