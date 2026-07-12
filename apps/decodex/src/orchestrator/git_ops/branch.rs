use std::{path::Path, process::Command};

use crate::{
	lane_authority::EffectReceipt,
	prelude::{Result, eyre},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LocalRefDeleteReadback {
	AlreadyAbsent(EffectReceipt),
	Deleted(EffectReceipt),
	PrerequisiteDrift { observed_oid: String },
}

#[allow(dead_code)]
pub(crate) fn delete_local_branch_at_oid(
	repo_root: &Path,
	branch_name: &str,
	expected_oid: &str,
	request_digest: &str,
	observed_at: &str,
	observed_at_unix: i64,
) -> Result<LocalRefDeleteReadback> {
	if [branch_name, expected_oid, request_digest].iter().any(|value| value.trim().is_empty()) {
		eyre::bail!("Local ref cleanup requires complete immutable identity.");
	}
	let valid = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["check-ref-format", "--branch", branch_name])
		.output()?;
	if !valid.status.success() {
		eyre::bail!("Local ref cleanup branch identity is invalid.");
	}
	let local_ref = format!("refs/heads/{branch_name}");
	let observed = local_ref_oid(repo_root, &local_ref)?;
	let Some(observed_oid) = observed else {
		return Ok(LocalRefDeleteReadback::AlreadyAbsent(local_ref_delete_receipt(
			branch_name,
			expected_oid,
			request_digest,
			observed_at,
			observed_at_unix,
		)?));
	};
	if observed_oid != expected_oid {
		return Ok(LocalRefDeleteReadback::PrerequisiteDrift { observed_oid });
	}
	let deleted = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["update-ref", "-d", local_ref.as_str(), expected_oid])
		.output()?;
	if !deleted.status.success() {
		eyre::bail!("Local ref compare-and-delete failed.");
	}
	if local_ref_oid(repo_root, &local_ref)?.is_some() {
		eyre::bail!("Local ref remained after compare-and-delete.");
	}
	Ok(LocalRefDeleteReadback::Deleted(local_ref_delete_receipt(
		branch_name,
		expected_oid,
		request_digest,
		observed_at,
		observed_at_unix,
	)?))
}

fn local_ref_oid(repo_root: &Path, local_ref: &str) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--hash", local_ref])
		.output()?;
	let stderr = String::from_utf8_lossy(&output.stderr);
	if output.status.code() == Some(1)
		|| stderr.contains("not a valid ref")
		|| stderr.contains("not found")
	{
		return Ok(None);
	}
	if !output.status.success() {
		eyre::bail!("Local ref readback failed.");
	}
	Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned()))
}

fn local_ref_delete_receipt(
	branch_name: &str,
	expected_oid: &str,
	request_digest: &str,
	observed_at: &str,
	observed_at_unix: i64,
) -> Result<EffectReceipt> {
	EffectReceipt::new(
		&format!("local-ref-delete:{branch_name}:{expected_oid}"),
		request_digest,
		expected_oid,
		None,
		Some(expected_oid),
		observed_at,
		observed_at_unix,
	)
}

pub(crate) fn delete_local_branch_if_present(repo_root: &Path, branch_name: &str) -> Result<()> {
	let local_ref = format!("refs/heads/{branch_name}");
	let branch_check = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", local_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect retained local branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let delete_output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["branch", "-D", branch_name])
		.output()?;

	if delete_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&delete_output.stderr);

	if stderr.contains("not found") || stderr.contains("branch not found") {
		return Ok(());
	}

	eyre::bail!(
		"Failed to delete retained local branch `{branch_name}` from `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

pub(crate) fn detach_worktree_head_from_branch_if_checked_out(
	worktree_path: &Path,
	branch_name: &str,
) -> Result<()> {
	let head_ref = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if !head_ref.status.success() {
		if head_ref.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&head_ref.stderr);

		eyre::bail!(
			"Failed to inspect retained worktree HEAD in `{}` before local branch cleanup: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let current_branch = String::from_utf8(head_ref.stdout)
		.map_err(|error| {
			eyre::eyre!(
				"Retained worktree HEAD in `{}` is not valid UTF-8: {error}",
				worktree_path.display()
			)
		})?
		.trim()
		.to_owned();

	if current_branch != branch_name {
		return Ok(());
	}

	let detach_output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["checkout", "--quiet", "--detach"])
		.output()?;

	if detach_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&detach_output.stderr);

	eyre::bail!(
		"Failed to detach retained worktree `{}` from branch `{branch_name}` before local branch cleanup: {}",
		worktree_path.display(),
		stderr.trim()
	);
}

#[cfg(test)]
mod tests {
	use std::fs;

	use tempfile::TempDir;

	use super::*;

	#[test]
	fn local_ref_delete_uses_expected_oid_cas_and_is_idempotent() {
		let fixture = repository();
		let oid = git(&fixture, &["rev-parse", "HEAD"]);
		git(&fixture, &["branch", "cleanup", &oid]);
		let deleted = delete_local_branch_at_oid(
			fixture.path(),
			"cleanup",
			&oid,
			"request",
			"2026-07-12T00:00:00Z",
			1,
		)
		.expect("delete");
		assert!(matches!(deleted, LocalRefDeleteReadback::Deleted(_)));
		let replay = delete_local_branch_at_oid(
			fixture.path(),
			"cleanup",
			&oid,
			"request",
			"2026-07-12T00:00:00Z",
			1,
		)
		.expect("replay");
		assert!(matches!(replay, LocalRefDeleteReadback::AlreadyAbsent(_)));
	}

	#[test]
	fn local_ref_delete_rejects_oid_drift_without_mutation() {
		let fixture = repository();
		let oid = git(&fixture, &["rev-parse", "HEAD"]);
		git(&fixture, &["branch", "cleanup", &oid]);
		let result = delete_local_branch_at_oid(
			fixture.path(),
			"cleanup",
			"0000000000000000000000000000000000000000",
			"request",
			"2026-07-12T00:00:00Z",
			1,
		)
		.expect("drift");
		assert_eq!(result, LocalRefDeleteReadback::PrerequisiteDrift { observed_oid: oid });
		assert!(!git(&fixture, &["show-ref", "--verify", "refs/heads/cleanup"]).is_empty());
	}

	fn repository() -> TempDir {
		let temp = TempDir::new().expect("tempdir");
		git(&temp, &["init", "-q"]);
		git(&temp, &["config", "user.email", "test@example.com"]);
		git(&temp, &["config", "user.name", "Test"]);
		fs::write(temp.path().join("README.md"), "fixture\n").expect("fixture");
		git(&temp, &["add", "README.md"]);
		git(
			&temp,
			&[
				"commit",
				"-q",
				"-m",
				r#"{"schema":"decodex/commit/2","change":"fixture","authority":"manual","impact":"compatible"}"#,
			],
		);
		temp
	}

	fn git(repo: &TempDir, args: &[&str]) -> String {
		let output =
			Command::new("git").arg("-C").arg(repo.path()).args(args).output().expect("git");
		assert!(output.status.success(), "git failed: {}", String::from_utf8_lossy(&output.stderr));
		String::from_utf8(output.stdout).expect("UTF-8").trim().to_owned()
	}
}
