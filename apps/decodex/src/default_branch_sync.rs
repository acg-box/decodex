use std::{collections::BTreeSet, path::Path, process::Command};

use crate::{
	git_credentials::{GitCredentialEnvironment, GitCredentialSource},
	prelude::{Result, eyre},
};

pub(crate) fn sync_repo_root_default_branch(
	repo_root: &Path,
	default_branch: &str,
	credentials: Option<GitCredentialSource<'_>>,
) -> Result<()> {
	let git_env = materialize_git_credentials(credentials);

	preflight_repo_root_default_branch_sync_with_env(repo_root, default_branch, &git_env)?;

	fast_forward_default_branch(repo_root, default_branch, &git_env)
}

pub(crate) fn preflight_repo_root_default_branch_sync(
	repo_root: &Path,
	default_branch: &str,
	credentials: Option<GitCredentialSource<'_>>,
) -> Result<()> {
	let git_env = materialize_git_credentials(credentials);

	preflight_repo_root_default_branch_sync_with_env(repo_root, default_branch, &git_env)
}

fn preflight_repo_root_default_branch_sync_with_env(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let current_branch = run_git_capture(repo_root, &["branch", "--show-current"], git_env)?;

	if current_branch.is_empty() {
		eyre::bail!(
			"Configured repo root `{}` is detached; landing closeout cannot fast-forward local `{default_branch}` there.",
			repo_root.display()
		);
	}
	if current_branch != default_branch {
		eyre::bail!(
			"Configured repo root `{}` is on branch `{current_branch}`, but landing closeout must fast-forward local `{default_branch}` there.",
			repo_root.display()
		);
	}

	ensure_clean_default_branch_worktree(repo_root, default_branch, git_env)?;
	fetch_default_branch(repo_root, default_branch, git_env)?;
	ensure_no_untracked_overwrite_conflicts(repo_root, default_branch, git_env)?;
	ensure_fast_forward_possible(repo_root, default_branch, git_env)?;

	Ok(())
}

fn ensure_clean_default_branch_worktree(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let status =
		run_git_capture(repo_root, &["status", "--porcelain", "--untracked-files=no"], git_env)?;

	if status.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` has tracked local changes; landing closeout cannot fast-forward local `{default_branch}` until they are cleared.",
		repo_root.display()
	);
}

fn fetch_default_branch(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let refspec = format!("refs/heads/{default_branch}:refs/remotes/origin/{default_branch}");

	run_git_checked(
		repo_root,
		&["fetch", "origin", refspec.as_str()],
		format!("fetch the latest `{default_branch}` from `origin`"),
		git_env,
	)
}

fn ensure_no_untracked_overwrite_conflicts(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let untracked_files =
		run_git_capture(repo_root, &["ls-files", "--others", "--exclude-standard"], git_env)?;

	if untracked_files.is_empty() {
		return Ok(());
	}

	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let incoming_paths = run_git_capture(
		repo_root,
		&["diff", "--name-only", "--diff-filter=ACMRTUXB", "HEAD", tracking_ref.as_str()],
		git_env,
	)?;

	if incoming_paths.is_empty() {
		return Ok(());
	}

	let incoming_paths = incoming_paths.lines().collect::<BTreeSet<_>>();
	let conflicting_paths = untracked_files
		.lines()
		.filter(|untracked_path| {
			incoming_paths.iter().any(|incoming_path| paths_conflict(untracked_path, incoming_path))
		})
		.map(str::to_owned)
		.collect::<Vec<_>>();

	if conflicting_paths.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"Configured repo root `{}` has untracked local files that would be overwritten by fast-forwarding `{default_branch}`: {}.",
		repo_root.display(),
		conflicting_paths.join(", ")
	);
}

fn paths_conflict(left: &str, right: &str) -> bool {
	left == right
		|| left.strip_prefix(right).is_some_and(|suffix| suffix.starts_with('/'))
		|| right.strip_prefix(left).is_some_and(|suffix| suffix.starts_with('/'))
}

fn ensure_fast_forward_possible(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");
	let status = build_git_command(
		repo_root,
		&["merge-base", "--is-ancestor", "HEAD", tracking_ref.as_str()],
		git_env,
	)
	.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` cannot fast-forward local `{default_branch}` to `{tracking_ref}` because local `{default_branch}` contains commits that are not on origin.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor HEAD {tracking_ref}` failed in `{}` with status `{}`.",
		repo_root.display(),
		status
	);
}

fn fast_forward_default_branch(
	repo_root: &Path,
	default_branch: &str,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let tracking_ref = format!("refs/remotes/origin/{default_branch}");

	run_git_checked(
		repo_root,
		&["merge", "--ff-only", tracking_ref.as_str()],
		format!("fast-forward local `{default_branch}` to `{tracking_ref}`"),
		git_env,
	)
}

fn run_git_capture(
	cwd: &Path,
	args: &[&str],
	git_env: &GitCredentialEnvironment,
) -> Result<String> {
	let output = build_git_command(cwd, args, git_env).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		eyre::bail!("`git {}` failed in `{}`: {detail}", args.join(" "), cwd.display());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git_checked(
	cwd: &Path,
	args: &[&str],
	action: String,
	git_env: &GitCredentialEnvironment,
) -> Result<()> {
	let output = build_git_command(cwd, args, git_env).output()?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!("Failed to {action} in `{}`.", cwd.display());
	}

	eyre::bail!("Failed to {action} in `{}`: {detail}", cwd.display());
}

fn build_git_command(cwd: &Path, args: &[&str], git_env: &GitCredentialEnvironment) -> Command {
	let mut command = Command::new("git");

	git_env.apply_to(&mut command);
	command.arg("-C").arg(cwd).args(args);

	command
}

fn materialize_git_credentials(
	credentials: Option<GitCredentialSource<'_>>,
) -> GitCredentialEnvironment {
	let Some(credentials) = credentials else {
		return GitCredentialEnvironment::default();
	};

	credentials.materialize_github_credentials()
}

#[cfg(test)]
mod tests {
	use std::{
		collections::HashMap,
		fs,
		path::{Path, PathBuf},
	};

	use tempfile::TempDir;

	use crate::{default_branch_sync, git_credentials::GitCredentialEnvironment, state};

	#[test]
	fn sync_repo_root_default_branch_fast_forwards_local_main() {
		let (_temp_dir, repo_root, remote_root) = init_repo();
		let peer_root = clone_repo(&remote_root, "peer");

		fs::write(peer_root.join("README.md"), "seed\nremote update\n")
			.expect("peer update should write");

		run_git(&peer_root, &["add", "README.md"]);
		run_git(&peer_root, &["commit", "-m", "remote update"]);
		run_git(&peer_root, &["push", "origin", "main"]);

		let before = git_stdout(&repo_root, &["rev-parse", "HEAD"]);

		default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
			.expect("repo root main should fast-forward");

		let after = git_stdout(&repo_root, &["rev-parse", "HEAD"]);
		let remote = git_stdout(&repo_root, &["rev-parse", "refs/remotes/origin/main"]);

		assert_ne!(before, after, "sync should advance local main");
		assert_eq!(after, remote, "local main should match origin/main after sync");
	}

	#[test]
	fn sync_repo_root_default_branch_rejects_non_default_branch_checkout() {
		let (_temp_dir, repo_root, _remote_root) = init_repo();

		run_git(&repo_root, &["checkout", "-b", "feature"]);

		let error = default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
			.expect_err("non-default repo root branch should be rejected");

		assert!(error.to_string().contains("is on branch `feature`"));
		assert!(error.to_string().contains("fast-forward local `main`"));
	}

	#[test]
	fn sync_repo_root_default_branch_rejects_tracked_local_changes() {
		let (_temp_dir, repo_root, _remote_root) = init_repo();

		fs::write(repo_root.join("README.md"), "dirty\n").expect("tracked change should write");

		let error = default_branch_sync::sync_repo_root_default_branch(&repo_root, "main", None)
			.expect_err("tracked dirty repo root should be rejected");

		assert!(error.to_string().contains("tracked local changes"));
	}

	#[test]
	fn preflight_repo_root_default_branch_sync_rejects_local_commits_not_on_origin() {
		let (_temp_dir, repo_root, _remote_root) = init_repo();

		fs::write(repo_root.join("README.md"), "seed\nlocal-only\n")
			.expect("local-only update should write");

		run_git(&repo_root, &["add", "README.md"]);
		run_git(&repo_root, &["commit", "-m", "local only"]);

		let error =
			default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
				.expect_err("local-only commits should block ff-only preflight");

		assert!(error.to_string().contains("cannot fast-forward local `main`"));
		assert!(error.to_string().contains("not on origin"));
	}

	#[test]
	fn preflight_repo_root_default_branch_sync_accepts_clean_default_branch_checkout() {
		let (_temp_dir, repo_root, _remote_root) = init_repo();

		default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
			.expect("clean repo root on the default branch should pass preflight");
	}

	#[test]
	fn preflight_repo_root_default_branch_sync_accepts_untracked_decodex_runtime_markers() {
		let (_temp_dir, repo_root, _remote_root) = init_repo();

		fs::write(repo_root.join(state::RUN_ACTIVITY_MARKER_FILE), "runtime marker\n")
			.expect("runtime marker should write");
		default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
			.expect("untracked Decodex activity marker should not block clean-source preflight");
	}

	#[test]
	fn default_branch_git_commands_use_routed_noninteractive_credentials() {
		let git_env = GitCredentialEnvironment::with_github_credentials(
			String::from("GITHUB_PAT_Y"),
			String::from("ghp_test_token"),
		);
		let command = default_branch_sync::build_git_command(
			Path::new("/repo"),
			&["fetch", "origin", "refs/heads/main:refs/remotes/origin/main"],
			&git_env,
		);
		let args =
			command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>();
		let envs = command
			.get_envs()
			.filter_map(|(key, value)| {
				Some((key.to_string_lossy().into_owned(), value?.to_string_lossy().into_owned()))
			})
			.collect::<HashMap<_, _>>();

		assert_eq!(
			args,
			["-C", "/repo", "fetch", "origin", "refs/heads/main:refs/remotes/origin/main"]
		);
		assert_eq!(envs.get("GH_TOKEN").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GITHUB_TOKEN").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GITHUB_PAT_Y").map(String::as_str), Some("ghp_test_token"));
		assert_eq!(envs.get("GH_PROMPT_DISABLED").map(String::as_str), Some("1"));
		assert_eq!(envs.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
		assert_eq!(envs.get("GCM_INTERACTIVE").map(String::as_str), Some("never"));
		assert!(!envs.contains_key("GIT_ASKPASS"));
		assert_eq!(envs.get("GIT_CONFIG_COUNT").map(String::as_str), Some("11"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_0").map(String::as_str), Some("credential.helper"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_0").map(String::as_str), Some(""));
		assert_eq!(envs.get("GIT_CONFIG_KEY_1").map(String::as_str), Some("credential.helper"));
		assert!(
			envs.get("GIT_CONFIG_VALUE_1").is_some_and(
				|value| value.contains("github.com") && value.contains("x-access-token")
			)
		);
		assert_eq!(
			envs.get("GIT_CONFIG_KEY_2").map(String::as_str),
			Some("url.https://github.com/.insteadOf")
		);
		assert_eq!(envs.get("GIT_CONFIG_VALUE_2").map(String::as_str), Some("git@github.com:"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_8").map(String::as_str), Some("commit.gpgsign"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_8").map(String::as_str), Some("false"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_9").map(String::as_str), Some("tag.gpgsign"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_9").map(String::as_str), Some("false"));
		assert_eq!(envs.get("GIT_CONFIG_KEY_10").map(String::as_str), Some("user.signingkey"));
		assert_eq!(envs.get("GIT_CONFIG_VALUE_10").map(String::as_str), Some(""));
	}

	#[test]
	fn preflight_repo_root_default_branch_sync_rejects_untracked_overwrite_conflicts() {
		let (_temp_dir, repo_root, remote_root) = init_repo();
		let peer_root = clone_repo(&remote_root, "peer");

		fs::write(peer_root.join("conflict.txt"), "remote tracked file\n")
			.expect("peer conflict file should write");

		run_git(&peer_root, &["add", "conflict.txt"]);
		run_git(&peer_root, &["commit", "-m", "add conflict file"]);
		run_git(&peer_root, &["push", "origin", "main"]);

		fs::write(repo_root.join("conflict.txt"), "local untracked file\n")
			.expect("repo-root untracked conflict file should write");

		let error =
			default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
				.expect_err("incoming tracked paths must not overwrite local untracked files");

		assert!(error.to_string().contains("untracked local files"));
		assert!(error.to_string().contains("conflict.txt"));
	}

	#[test]
	fn preflight_repo_root_default_branch_sync_rejects_untracked_path_prefix_conflicts() {
		let (_temp_dir, repo_root, remote_root) = init_repo();
		let peer_root = clone_repo(&remote_root, "peer");

		fs::create_dir_all(peer_root.join("docs")).expect("peer nested directory should exist");
		fs::write(peer_root.join("docs/guide.md"), "remote tracked file\n")
			.expect("peer nested file should write");

		run_git(&peer_root, &["add", "docs/guide.md"]);
		run_git(&peer_root, &["commit", "-m", "add nested file"]);
		run_git(&peer_root, &["push", "origin", "main"]);

		fs::write(repo_root.join("docs"), "local untracked file\n")
			.expect("repo-root conflicting untracked file should write");

		let error =
			default_branch_sync::preflight_repo_root_default_branch_sync(&repo_root, "main", None)
				.expect_err(
					"incoming tracked directories must not overwrite local untracked files",
				);

		assert!(error.to_string().contains("untracked local files"));
		assert!(error.to_string().contains("docs"));
	}

	fn init_repo() -> (TempDir, PathBuf, PathBuf) {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let repo_root = temp_dir.path().join("repo");
		let remote_root = temp_dir.path().join("origin.git");

		fs::create_dir_all(&repo_root).expect("repo root should exist");

		run_git(
			temp_dir.path(),
			&[
				"init",
				"--bare",
				"--initial-branch",
				"main",
				remote_root.to_str().expect("remote path utf-8"),
			],
		);
		run_git(&repo_root, &["init", "--initial-branch", "main"]);
		run_git(&repo_root, &["config", "user.name", "Decodex Tests"]);
		run_git(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
		run_git(&repo_root, &["config", "commit.gpgsign", "false"]);
		run_git(&repo_root, &["config", "tag.gpgsign", "false"]);
		run_git(
			&repo_root,
			&["remote", "add", "origin", remote_root.to_str().expect("remote path utf-8")],
		);

		fs::write(repo_root.join("README.md"), "seed\n").expect("seed file should write");

		run_git(&repo_root, &["add", "README.md"]);
		run_git(&repo_root, &["commit", "-m", "seed"]);
		run_git(&repo_root, &["push", "-u", "origin", "main"]);

		(temp_dir, repo_root, remote_root)
	}

	fn clone_repo(remote_root: &Path, name: &str) -> PathBuf {
		let clone_root = remote_root.parent().expect("remote should have parent").join(name);

		run_git(
			remote_root.parent().expect("remote should have parent"),
			&[
				"clone",
				remote_root.to_str().expect("remote path utf-8"),
				clone_root.to_str().expect("clone path utf-8"),
			],
		);
		run_git(&clone_root, &["config", "user.name", "Decodex Tests"]);
		run_git(&clone_root, &["config", "user.email", "decodex-tests@example.com"]);
		run_git(&clone_root, &["config", "commit.gpgsign", "false"]);

		clone_root
	}

	fn git_stdout(cwd: &Path, args: &[&str]) -> String {
		let output = crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(cwd)
			.args(args)
			.output()
			.expect("git should run");

		assert!(
			output.status.success(),
			"git {:?} failed in `{}`: {}",
			args,
			cwd.display(),
			String::from_utf8_lossy(&output.stderr)
		);

		String::from_utf8_lossy(&output.stdout).trim().to_owned()
	}

	fn run_git(cwd: &Path, args: &[&str]) {
		let status = crate::test_support::hermetic_git_command()
			.arg("-C")
			.arg(cwd)
			.args(args)
			.status()
			.expect("git should run");

		assert!(status.success(), "git {:?} should succeed in `{}`", args, cwd.display());
	}
}
