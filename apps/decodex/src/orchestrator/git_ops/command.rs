use std::{
	collections::BTreeSet,
	env,
	ffi::{OsStr, OsString},
	path::Path,
	process::{Command, Output},
};

use color_eyre::Report;

use crate::{
	orchestrator::git_ops::{
		self, RepoGateFailure, RepoGateFailureDiagnostic, RepoGateFailureKind, diagnostic,
	},
	prelude::{Result, eyre},
};

pub(crate) fn repo_gate_shell_from_env(shell: Option<OsString>) -> (OsString, &'static str) {
	if let Some(shell) = shell
		&& !shell.is_empty()
	{
		let shell_path = Path::new(&shell);
		let shell_name = shell_path.file_name().and_then(OsStr::to_str);

		if shell_name == Some("sh") {
			return (OsString::from("/bin/sh"), "-c");
		}
		if !shell_path.is_absolute() || shell_path.is_file() {
			return (shell, "-lc");
		}
	}

	(OsString::from("/bin/sh"), "-c")
}

pub(crate) fn run_repo_gate_cleanliness_check_with_git(
	git_binary: &OsStr,
	cwd: &Path,
) -> Result<Output> {
	Command::new(git_binary)
		.arg("-C")
		.arg(cwd)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn repo gate tracked-file cleanliness check in `{}` via `{}`: {}",
					cwd.display(),
					git_binary.to_string_lossy(),
					error
				),
			))
		})
}

pub(crate) fn repo_gate_changed_tracked_files(cwd: &Path) -> Result<BTreeSet<String>> {
	let base_ref = repo_gate_remote_head_ref(cwd)?;
	let merge_base = repo_gate_merge_base(cwd, &base_ref)?;
	let committed_range = format!("{merge_base}..HEAD");
	let mut changed_files = repo_gate_changed_files_for_diff_spec(cwd, &committed_range)?;

	changed_files.extend(repo_gate_changed_files_for_diff_spec(cwd, "HEAD")?);

	Ok(changed_files)
}

pub(super) fn run_repo_gate_shell_command(command: &str, cwd: &Path) -> Result<Output> {
	let (shell, shell_flag) = repo_gate_shell();

	Command::new(&shell).arg(shell_flag).arg(command).current_dir(cwd).output().map_err(|error| {
		let diagnostic = RepoGateFailureDiagnostic::from_spawn_error("spawn", command, &error);

		Report::new(
			RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn repo gate command `{}` in `{}` via `{}` `{}`: {}",
					command,
					cwd.display(),
					shell.to_string_lossy(),
					shell_flag,
					error
				),
			)
			.with_diagnostic(diagnostic),
		)
	})
}

fn repo_gate_shell() -> (OsString, &'static str) {
	repo_gate_shell_from_env(env::var_os("SHELL"))
}

fn run_repo_gate_git_command(args: &[&str], cwd: &Path) -> Result<Output> {
	Command::new("git").arg("-C").arg(cwd).args(args).output().map_err(|error| {
		eyre::eyre!(
			"Failed to inspect repo-gate changed-file classification in `{}` via `git {}`: {}",
			cwd.display(),
			args.join(" "),
			error
		)
	})
}

fn repo_gate_remote_head_ref(cwd: &Path) -> Result<String> {
	let output = run_repo_gate_git_command(
		&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"],
		cwd,
	)?;

	if output.status.success() {
		let remote_head = git_ops::repo_gate_output_text(&output);

		if !remote_head.is_empty() && remote_head != "(command produced no output)" {
			return Ok(remote_head);
		}
	}

	let remote_probe =
		run_repo_gate_git_command(&["ls-remote", "--symref", "origin", "HEAD"], cwd)?;

	if !remote_probe.status.success() {
		eyre::bail!(
			"Failed to resolve `origin/HEAD` for repo-gate changed-file classification in `{}`: {}",
			cwd.display(),
			git_ops::repo_gate_output_text(&remote_probe)
		);
	}

	let stdout = String::from_utf8_lossy(&remote_probe.stdout);
	let Some(remote_head) = stdout.lines().find_map(|line| {
		let line = line.trim();

		line.strip_prefix("ref: refs/heads/")
			.and_then(|remainder| remainder.strip_suffix("\tHEAD"))
			.map(|branch_name| format!("origin/{branch_name}"))
	}) else {
		eyre::bail!(
			"Remote `origin` did not advertise a default HEAD branch for repo-gate changed-file classification in `{}`.",
			cwd.display()
		);
	};

	Ok(remote_head)
}

fn repo_gate_merge_base(cwd: &Path, base_ref: &str) -> Result<String> {
	let output = run_repo_gate_git_command(&["merge-base", "HEAD", base_ref], cwd)?;

	if !output.status.success() {
		eyre::bail!(
			"Failed to resolve merge-base for repo-gate changed-file classification in `{}` against `{}`: {}",
			cwd.display(),
			base_ref,
			git_ops::repo_gate_output_text(&output)
		);
	}

	let merge_base = git_ops::repo_gate_output_text(&output);

	if merge_base.is_empty() || merge_base == "(command produced no output)" {
		eyre::bail!(
			"`git merge-base` returned no revision for repo-gate changed-file classification in `{}` against `{}`.",
			cwd.display(),
			base_ref
		);
	}

	Ok(merge_base)
}

fn repo_gate_changed_files_for_diff_spec(cwd: &Path, diff_spec: &str) -> Result<BTreeSet<String>> {
	let output = run_repo_gate_git_command(
		&["diff", "--name-only", "--diff-filter=ACDMRTUXB", diff_spec],
		cwd,
	)?;

	if !output.status.success() {
		eyre::bail!(
			"Failed to compute repo-gate changed-file classification in `{}` for diff `{}`: {}",
			cwd.display(),
			diff_spec,
			git_ops::repo_gate_output_text(&output)
		);
	}

	Ok(diagnostic::repo_gate_git_output_lines(&output))
}
