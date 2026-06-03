#[cfg(unix)] use std::os::{fd::AsRawFd, unix::process::CommandExt as _};
use std::{
	env,
	ffi::OsStr,
	fs,
	io::{Error, ErrorKind, Read},
	path::{Path, PathBuf},
	process::{Command, Output, Stdio},
	thread,
	time::{Duration, Instant},
};

use libc::{ESRCH, F_GETFL, F_SETFL, O_NONBLOCK, SIGKILL};

use crate::{
	prelude::{Result, eyre},
	state::{self, RUN_ACTIVITY_MARKER_FILE},
	workflow::WorkflowWorkspaceHooks,
};

const AFTER_CREATE_PENDING_MARKER: &str = ".decodex-after-create.pending";
const WORKSPACE_HOOK_CAPTURE_LIMIT: usize = 1_024 * 1_024;
const WORKSPACE_HOOK_TRUNCATED_MARKER: &[u8] = b"\n[decodex truncated workspace hook output]\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorktreeSpec {
	pub(crate) branch_name: String,
	pub(crate) issue_identifier: String,
	pub(crate) path: PathBuf,
	pub(crate) reused_existing: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MergedWorktreeCleanupDebt {
	pub(crate) branch_name: String,
	pub(crate) cleanliness: MergedWorktreeCleanliness,
	pub(crate) default_branch: String,
	pub(crate) path: PathBuf,
}

pub(crate) struct WorktreeManager {
	repo_root: PathBuf,
	worktree_root: PathBuf,
	project_id: String,
}
impl WorktreeManager {
	pub(crate) fn new(
		project_id: impl Into<String>,
		repo_root: impl Into<PathBuf>,
		worktree_root: impl Into<PathBuf>,
	) -> Self {
		Self {
			repo_root: repo_root.into(),
			worktree_root: worktree_root.into(),
			project_id: project_id.into(),
		}
	}

	pub(crate) fn plan_for_issue(&self, issue_identifier: &str) -> WorktreeSpec {
		let branch_suffix = sanitize_branch_component(issue_identifier);
		let branch_owner =
			configured_branch_owner(&self.repo_root).unwrap_or_else(|| String::from("x"));
		let branch_name = format!(
			"{}/{}-{}",
			sanitize_branch_component(&branch_owner),
			sanitize_branch_component(&self.project_id),
			branch_suffix
		);
		let path = self.worktree_root.join(issue_identifier);
		let reused_existing = path.join(".git").exists();

		WorktreeSpec {
			branch_name,
			issue_identifier: issue_identifier.to_owned(),
			path,
			reused_existing,
		}
	}

	#[cfg(test)]
	pub(crate) fn ensure_worktree(
		&self,
		issue_identifier: &str,
		dry_run: bool,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, None)
	}

	pub(crate) fn ensure_worktree_with_hooks(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<WorktreeSpec> {
		self.ensure_worktree_internal(issue_identifier, dry_run, Some(hooks))
	}

	fn ensure_worktree_internal(
		&self,
		issue_identifier: &str,
		dry_run: bool,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<WorktreeSpec> {
		let spec = self.plan_for_issue(issue_identifier);

		if dry_run {
			return Ok(spec);
		}
		if spec.reused_existing {
			self.validate_worktree_boundary(&spec.path)?;

			normalize_origin_remote_for_worktrees(&self.repo_root)?;

			self.resume_after_create_hooks_if_pending(&spec, hooks)?;

			return Ok(spec);
		}

		fs::create_dir_all(&self.worktree_root)?;

		self.create_linked_worktree(&spec, hooks)?;
		self.validate_worktree_boundary(&spec.path)?;
		self.run_after_create_hooks(&spec, hooks)?;

		Ok(spec)
	}

	#[cfg(test)]
	pub(crate) fn remove_worktree_path(&self, path: &Path) -> Result<bool> {
		self.remove_worktree_path_internal(path, None)
	}

	pub(crate) fn remove_worktree_path_with_hooks(
		&self,
		issue_identifier: &str,
		branch_name: &str,
		path: &Path,
		hooks: &WorkflowWorkspaceHooks,
	) -> Result<bool> {
		self.remove_worktree_path_internal(path, Some((issue_identifier, branch_name, hooks)))
	}

	fn remove_worktree_path_internal(
		&self,
		path: &Path,
		hooks: Option<(&str, &str, &WorkflowWorkspaceHooks)>,
	) -> Result<bool> {
		if !path.exists() {
			return Ok(false);
		}

		let worktree_root = fs::canonicalize(&self.worktree_root)?;
		let canonical_path = fs::canonicalize(path)?;

		if !canonical_path.starts_with(&worktree_root) || canonical_path == worktree_root {
			eyre::bail!(
				"Refusing to remove worktree `{}` outside worktree_root `{}`.",
				path.display(),
				self.worktree_root.display()
			);
		}
		if remove_orphan_marker_directory_if_safe(&canonical_path)? {
			return Ok(true);
		}

		self.validate_worktree_boundary(&canonical_path)?;

		if let Some((issue_identifier, branch_name, hooks)) = hooks {
			self.run_workspace_hook_phase(
				"before_remove",
				issue_identifier,
				branch_name,
				&canonical_path,
				hooks.before_remove_commands(),
				hooks.timeout_seconds(),
			)?;
		}

		run_git(
			&self.repo_root,
			[
				"worktree",
				"remove",
				"--force",
				canonical_path.as_os_str().to_str().ok_or_else(|| {
					eyre::eyre!("Worktree path `{}` is not valid UTF-8.", canonical_path.display())
				})?,
			],
			"remove the linked worktree",
		)?;

		Ok(true)
	}

	fn create_linked_worktree(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let source_head =
			git_stdout(&self.repo_root, ["rev-parse", "HEAD"], "read the source repository HEAD")?;

		if spec.path.exists() {
			eyre::bail!(
				"Worktree path `{}` already exists but does not look reusable.",
				spec.path.display()
			);
		}

		let create_output = Command::new("git")
			.arg("-C")
			.arg(&self.repo_root)
			.args(["worktree", "add", "--quiet", "--detach"])
			.arg(&spec.path)
			.arg(&source_head)
			.output()?;

		if !create_output.status.success() {
			let stderr = String::from_utf8_lossy(&create_output.stderr);

			eyre::bail!(
				"Failed to create linked worktree `{}` from `{}`: {}",
				spec.path.display(),
				self.repo_root.display(),
				stderr.trim()
			);
		}

		let setup_result = normalize_origin_remote_for_worktrees(&self.repo_root).and_then(|_| {
			self.checkout_worktree_branch(&spec.path, spec.branch_name.as_str(), &source_head)
		});

		if let Err(error) = setup_result {
			let _ = self.remove_worktree_path_internal(&spec.path, None);

			return Err(error);
		}

		if workspace_requires_after_create_pending_marker(hooks) {
			let pending_marker = after_create_pending_marker_path(&spec.path);

			fs::write(&pending_marker, b"pending\n").map_err(|error| {
				let _ = self.remove_worktree_path_internal(&spec.path, None);

				eyre::eyre!(
					"Failed to write pending after-create marker `{}`: {error}",
					pending_marker.display()
				)
			})?;
		}

		Ok(())
	}

	fn checkout_worktree_branch(
		&self,
		worktree_path: &Path,
		branch_name: &str,
		source_head: &str,
	) -> Result<()> {
		if fetch_remote_branch_if_present(&self.repo_root, branch_name)? {
			let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");

			run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, remote_tracking_ref.as_str()],
				"checkout the worktree branch from the remote lane head",
			)?;
		} else {
			run_git(
				worktree_path,
				["checkout", "--quiet", "-B", branch_name, source_head],
				"checkout the worktree branch",
			)?;
		}

		Ok(())
	}

	fn validate_worktree_boundary(&self, worktree_path: &Path) -> Result<()> {
		let git_pointer = worktree_path.join(".git");

		if !git_pointer.is_file() {
			eyre::bail!(
				"Worktree `{}` is not a linked git worktree: expected `.git` to be a pointer file.",
				worktree_path.display()
			);
		}

		let repo_git_dir = resolve_source_repo_git_common_dir(&self.repo_root)?;
		let worktree_admin_root = repo_git_dir.join("worktrees");
		let canonical_worktree_path = fs::canonicalize(worktree_path)?;
		let git_dir = fs::canonicalize(PathBuf::from(git_stdout(
			worktree_path,
			["rev-parse", "--path-format=absolute", "--git-dir"],
			"resolve worktree git dir",
		)?))?;
		let git_common_dir = fs::canonicalize(PathBuf::from(git_stdout(
			worktree_path,
			["rev-parse", "--path-format=absolute", "--git-common-dir"],
			"resolve worktree git common dir",
		)?))?;

		if !git_dir.starts_with(&worktree_admin_root) {
			eyre::bail!(
				"Worktree `{}` is not linked through `{}`: git dir resolved to `{}`.",
				worktree_path.display(),
				worktree_admin_root.display(),
				git_dir.display()
			);
		}
		if git_common_dir != repo_git_dir {
			eyre::bail!(
				"Worktree `{}` must share git common dir `{}`, found `{}`.",
				worktree_path.display(),
				repo_git_dir.display(),
				git_common_dir.display()
			);
		}
		if !worktree_is_registered(&self.repo_root, &canonical_worktree_path)? {
			eyre::bail!(
				"Worktree `{}` is not registered with the source repository worktree admin.",
				worktree_path.display()
			);
		}

		Ok(())
	}

	fn run_workspace_hook_phase(
		&self,
		phase_name: &str,
		issue_identifier: &str,
		branch_name: &str,
		worktree_path: &Path,
		commands: &[String],
		timeout_seconds: u64,
	) -> Result<()> {
		if commands.is_empty() {
			return Ok(());
		}

		let envs = [
			("DECODEX_REPO_ROOT", self.repo_root.display().to_string()),
			("DECODEX_WORKTREE_PATH", worktree_path.display().to_string()),
			("DECODEX_ISSUE_ID", issue_identifier.to_owned()),
			("DECODEX_BRANCH", branch_name.to_owned()),
		];

		for command in commands {
			let output = run_workspace_hook_shell_command(
				command,
				worktree_path,
				&envs,
				Duration::from_secs(timeout_seconds),
			)
			.map_err(|error| {
				eyre::eyre!(
					"Failed to run workspace hook `{phase_name}` command `{command}` in `{}`: {error}",
					worktree_path.display()
				)
			})?;

			if !output.status.success() {
				let mut details = String::new();

				append_output_details(&mut details, &output);

				eyre::bail!(
					"Workspace hook `{phase_name}` command `{command}` failed in `{}` with status `{}`.{details}",
					worktree_path.display(),
					output.status
				);
			}
		}

		Ok(())
	}

	fn run_after_create_hooks(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let Some(hooks) = hooks else {
			return Ok(());
		};

		if hooks.after_create_commands().is_empty() {
			return Ok(());
		}

		let pending_marker = after_create_pending_marker_path(&spec.path);

		if let Err(error) = self.run_workspace_hook_phase(
			"after_create",
			spec.issue_identifier.as_str(),
			spec.branch_name.as_str(),
			&spec.path,
			hooks.after_create_commands(),
			hooks.timeout_seconds(),
		) {
			fs::write(&pending_marker, b"pending\n").map_err(|marker_error| {
				eyre::eyre!(
					"Workspace after-create hook failed and pending marker `{}` could not be restored: {marker_error}. Original error: {error}",
					pending_marker.display()
				)
			})?;

			return Err(error);
		}
		if let Err(error) = fs::remove_file(&pending_marker)
			&& error.kind() != ErrorKind::NotFound
		{
			return Err(eyre::eyre!(
				"Failed to clear pending after-create marker `{}` after successful bootstrap: {error}",
				pending_marker.display()
			));
		}

		Ok(())
	}

	fn resume_after_create_hooks_if_pending(
		&self,
		spec: &WorktreeSpec,
		hooks: Option<&WorkflowWorkspaceHooks>,
	) -> Result<()> {
		let Some(hooks) = hooks else {
			return Ok(());
		};

		if hooks.after_create_commands().is_empty() {
			return Ok(());
		}
		if !after_create_pending_marker_path(&spec.path).exists() {
			return Ok(());
		}

		self.run_after_create_hooks(spec, Some(hooks))
	}
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
	Ok(parse_linked_worktrees(&git_stdout(
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
	let branch_tip = git_stdout(repo_root, ["rev-parse", branch_ref], "resolve branch tip")?;
	let first_parent_history = git_stdout(
		repo_root,
		["rev-list", "--first-parent", default_ref],
		"list default branch first-parent history",
	)?;

	Ok(first_parent_history.lines().any(|commit| commit == branch_tip))
}

fn git_refs_point_to_same_tip(repo_root: &Path, left_ref: &str, right_ref: &str) -> Result<bool> {
	let left_tip = git_stdout(repo_root, ["rev-parse", left_ref], "resolve git ref tip")?;
	let right_tip = git_stdout(repo_root, ["rev-parse", right_ref], "resolve git ref tip")?;

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

fn configured_branch_owner(repo_root: &Path) -> Option<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["config", "--get", "codex.github-identity"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();

	(!value.is_empty()).then_some(value)
}

fn worktree_is_registered(repo_root: &Path, expected_path: &Path) -> Result<bool> {
	let output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["worktree", "list", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to list linked worktrees in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	for line in String::from_utf8_lossy(&output.stdout).lines() {
		let Some(path) = line.strip_prefix("worktree ") else {
			continue;
		};
		let candidate = fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

		if candidate == expected_path {
			return Ok(true);
		}
	}

	Ok(false)
}

fn resolve_source_repo_git_common_dir(repo_root: &Path) -> Result<PathBuf> {
	Ok(fs::canonicalize(PathBuf::from(git_stdout(
		repo_root,
		["rev-parse", "--path-format=absolute", "--git-common-dir"],
		"resolve source repository git common dir",
	)?))?)
}

fn after_create_pending_marker_path(worktree_path: &Path) -> PathBuf {
	worktree_path.join(AFTER_CREATE_PENDING_MARKER)
}

fn workspace_requires_after_create_pending_marker(hooks: Option<&WorkflowWorkspaceHooks>) -> bool {
	hooks.is_some_and(|hooks| !hooks.after_create_commands().is_empty())
}

fn remove_orphan_marker_directory_if_safe(path: &Path) -> Result<bool> {
	if path.join(".git").exists() || !path.is_dir() {
		return Ok(false);
	}

	let mut has_marker = false;

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let file_type = entry.file_type()?;
		let file_name = entry.file_name();
		let Some(file_name) = file_name.to_str() else {
			return Ok(false);
		};

		if !file_type.is_file() {
			return Ok(false);
		}

		match file_name {
			RUN_ACTIVITY_MARKER_FILE | AFTER_CREATE_PENDING_MARKER => has_marker = true,
			_ => return Ok(false),
		}
	}

	if !has_marker {
		return Ok(false);
	}

	fs::remove_dir_all(path)?;

	Ok(true)
}

fn workspace_hook_shell_from_env(
	shell: Option<std::ffi::OsString>,
) -> (std::ffi::OsString, &'static str) {
	if let Some(shell) = shell
		&& !shell.is_empty()
	{
		let shell_path = Path::new(&shell);
		let shell_name = shell_path.file_name().and_then(OsStr::to_str);

		if shell_name == Some("sh") {
			return (std::ffi::OsString::from("/bin/sh"), "-c");
		}
		if !shell_path.is_absolute() || shell_path.is_file() {
			return (shell, "-lc");
		}
	}

	(std::ffi::OsString::from("/bin/sh"), "-c")
}

#[cfg(unix)]
fn workspace_hook_shell() -> (std::ffi::OsString, &'static str) {
	workspace_hook_shell_from_env(env::var_os("SHELL"))
}

#[cfg(unix)]
fn run_workspace_hook_shell_command(
	command: &str,
	cwd: &Path,
	envs: &[(&str, String)],
	timeout: Duration,
) -> Result<Output> {
	let (shell, shell_flag) = workspace_hook_shell();
	let deadline = Instant::now() + timeout;
	let mut shell_command = Command::new(&shell);

	shell_command
		.arg(shell_flag)
		.arg(command)
		.current_dir(cwd)
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.envs(envs.iter().map(|(key, value)| (*key, value.as_str())));
	unsafe {
		shell_command.pre_exec(|| {
			if libc::setpgid(0, 0) == -1 {
				return Err(Error::last_os_error());
			}

			Ok(())
		});
	}

	let mut child = shell_command.spawn().map_err(|error| {
		eyre::eyre!(
			"Failed to spawn workspace hook shell command `{command}` in `{}` via `{}` `{}`: {error}",
			cwd.display(),
			shell.to_string_lossy(),
			shell_flag
		)
	})?;
	let stdout_reader = child.stdout.take().ok_or_else(|| {
		eyre::eyre!(
			"Failed to capture stdout for workspace hook shell command `{command}` in `{}`.",
			cwd.display()
		)
	})?;
	let stderr_reader = child.stderr.take().ok_or_else(|| {
		eyre::eyre!(
			"Failed to capture stderr for workspace hook shell command `{command}` in `{}`.",
			cwd.display()
		)
	})?;
	let mut stdout_reader = stdout_reader;
	let mut stderr_reader = stderr_reader;

	configure_nonblocking_pipe(&stdout_reader, "stdout")?;
	configure_nonblocking_pipe(&stderr_reader, "stderr")?;

	let mut stdout = Vec::new();
	let mut stderr = Vec::new();

	loop {
		drain_pipe_nonblocking(&mut stdout_reader, &mut stdout, "stdout")?;
		drain_pipe_nonblocking(&mut stderr_reader, &mut stderr, "stderr")?;

		if let Some(status) = child.try_wait()? {
			drain_pipe_nonblocking(&mut stdout_reader, &mut stdout, "stdout")?;
			drain_pipe_nonblocking(&mut stderr_reader, &mut stderr, "stderr")?;

			return Ok(Output { status, stdout, stderr });
		}

		if Instant::now() >= deadline {
			let process_group_cleanup = kill_workspace_hook_process_group(child.id());
			let _ = child.kill();
			let status = child.wait()?;

			drain_pipe_nonblocking(&mut stdout_reader, &mut stdout, "stdout")?;
			drain_pipe_nonblocking(&mut stderr_reader, &mut stderr, "stderr")?;

			let output = Output { status, stdout, stderr };
			let mut details = String::new();

			append_output_details(&mut details, &output);
			append_process_group_cleanup_details(&mut details, process_group_cleanup);

			eyre::bail!(
				"Workspace hook shell command `{command}` in `{}` exceeded the {}s timeout.{details}",
				cwd.display(),
				timeout.as_secs()
			);
		}

		thread::sleep(Duration::from_millis(25));
	}
}

#[cfg(unix)]
fn configure_nonblocking_pipe<R>(reader: &R, stream_name: &str) -> Result<()>
where
	R: AsRawFd,
{
	let fd = reader.as_raw_fd();
	let flags = unsafe { libc::fcntl(fd, F_GETFL) };

	if flags == -1 {
		return Err(eyre::eyre!(
			"Failed to read workspace hook {stream_name} flags: {}",
			std::io::Error::last_os_error()
		));
	}
	if flags & O_NONBLOCK != 0 {
		return Ok(());
	}

	let result = unsafe { libc::fcntl(fd, F_SETFL, flags | O_NONBLOCK) };

	if result == -1 {
		return Err(eyre::eyre!(
			"Failed to set workspace hook {stream_name} pipe nonblocking: {}",
			std::io::Error::last_os_error()
		));
	}

	Ok(())
}

#[cfg(unix)]
fn kill_workspace_hook_process_group(process_id: u32) -> Result<()> {
	let process_group_id = i32::try_from(process_id).map_err(|error| {
		eyre::eyre!("Workspace hook process id `{process_id}` is out of range: {error}")
	})?;
	let result = unsafe { libc::killpg(process_group_id, SIGKILL) };

	if result == -1 {
		let error = Error::last_os_error();

		if error.raw_os_error() == Some(ESRCH) {
			return Ok(());
		}

		return Err(eyre::eyre!(
			"Failed to terminate workspace hook process group `{process_group_id}`: {error}"
		));
	}

	Ok(())
}

#[cfg(unix)]
fn drain_pipe_nonblocking<R>(reader: &mut R, buffer: &mut Vec<u8>, stream_name: &str) -> Result<()>
where
	R: Read,
{
	loop {
		let mut chunk = [0_u8; 8 * 1_024];

		match reader.read(&mut chunk) {
			Ok(0) => return Ok(()),
			Ok(read) => append_capped_workspace_hook_output(buffer, &chunk[..read]),
			Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => {
				return Err(eyre::eyre!("Failed to read workspace hook {stream_name}: {error}"));
			},
		}
	}
}

fn append_capped_workspace_hook_output(buffer: &mut Vec<u8>, chunk: &[u8]) {
	if buffer.len() >= WORKSPACE_HOOK_CAPTURE_LIMIT {
		return;
	}

	let remaining = WORKSPACE_HOOK_CAPTURE_LIMIT - buffer.len();

	if chunk.len() <= remaining {
		buffer.extend_from_slice(chunk);

		return;
	}

	let marker_len = remaining.min(WORKSPACE_HOOK_TRUNCATED_MARKER.len());
	let chunk_len = remaining.saturating_sub(marker_len);

	buffer.extend_from_slice(&chunk[..chunk_len]);
	buffer.extend_from_slice(&WORKSPACE_HOOK_TRUNCATED_MARKER[..marker_len]);
}

fn append_output_details(buffer: &mut String, output: &Output) {
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();

	if !stdout.is_empty() {
		buffer.push_str(&format!(" stdout: `{stdout}`."));
	}
	if !stderr.is_empty() {
		buffer.push_str(&format!(" stderr: `{stderr}`."));
	}
}

fn append_process_group_cleanup_details(buffer: &mut String, cleanup_result: Result<()>) {
	if let Err(error) = cleanup_result {
		buffer.push_str(&format!(" process-group cleanup error: `{error}`."));
	}
}

fn git_stdout<I, S>(repo_root: &Path, args: I, action: &str) -> Result<String>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn try_git_stdout<I, S>(repo_root: &Path, args: I, action: &str) -> Result<Option<String>>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if output.status.success() {
		return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_owned()));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	if stderr.contains("No such remote") {
		return Ok(None);
	}

	eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
}

fn run_git<I, S>(repo_root: &Path, args: I, action: &str) -> Result<()>
where
	I: IntoIterator<Item = S>,
	S: AsRef<OsStr>,
{
	let output = Command::new("git").arg("-C").arg(repo_root).args(args).output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!("Failed to {action} in `{}`: {}", repo_root.display(), stderr.trim());
	}

	Ok(())
}

fn normalize_origin_remote_for_worktrees(repo_root: &Path) -> Result<()> {
	let Some(origin_url) = try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	else {
		return Ok(());
	};

	if !is_relative_filesystem_remote(origin_url.as_str()) {
		return Ok(());
	}

	let absolute_origin = fs::canonicalize(repo_root.join(&origin_url))?;
	let absolute_origin = absolute_origin.to_str().ok_or_else(|| {
		eyre::eyre!(
			"Resolved absolute origin path `{}` is not valid UTF-8.",
			absolute_origin.display()
		)
	})?;

	run_git(
		repo_root,
		["remote", "set-url", "origin", absolute_origin],
		"normalize the source repository origin remote for linked worktrees",
	)
}

fn is_relative_filesystem_remote(remote_url: &str) -> bool {
	if remote_url.starts_with("./") || remote_url.starts_with("../") {
		return true;
	}
	if remote_url == "~" || remote_url.starts_with("~/") {
		return false;
	}

	!remote_url.contains("://") && !remote_url.contains(':') && !Path::new(remote_url).is_absolute()
}

fn configure_noninteractive_git(command: &mut Command) -> &mut Command {
	command.env("GIT_TERMINAL_PROMPT", "0").env("GCM_INTERACTIVE", "never")
}

fn fetch_remote_branch_if_present(repo_root: &Path, branch_name: &str) -> Result<bool> {
	if try_git_stdout(
		repo_root,
		["remote", "get-url", "origin"],
		"read source repository origin remote",
	)?
	.is_none()
	{
		return Ok(false);
	}

	let remote_ref = format!("refs/heads/{branch_name}");
	let mut branch_check = Command::new("git");

	configure_noninteractive_git(&mut branch_check);

	let branch_check = branch_check
		.arg("-C")
		.arg(repo_root)
		.args(["ls-remote", "--exit-code", "--heads", "origin", remote_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(2) {
			return Ok(false);
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect remote worktree branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let remote_tracking_ref = format!("refs/remotes/origin/{branch_name}");
	let mut fetch = Command::new("git");

	configure_noninteractive_git(&mut fetch);

	let output = fetch
		.arg("-C")
		.arg(repo_root)
		.args([
			"fetch",
			"--quiet",
			"--no-tags",
			"origin",
			&format!("refs/heads/{branch_name}:{remote_tracking_ref}"),
		])
		.output()?;

	if output.status.success() {
		return Ok(true);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to fetch remote worktree branch `{branch_name}` in `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

fn sanitize_branch_component(value: &str) -> String {
	value
		.chars()
		.map(|ch| match ch {
			'A'..='Z' => ch.to_ascii_lowercase(),
			'a'..='z' | '0'..='9' => ch,
			'-' | '_' => '-',
			_ => '-',
		})
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		path::{Path, PathBuf},
		process::Command,
		thread,
		time::{Duration, Instant},
	};

	use tempfile::TempDir;

	use crate::{git_credentials, workflow::WorkflowDocument, worktree::WorktreeManager};

	fn workspace_hooks(
		workspace_hooks_frontmatter: &str,
	) -> crate::workflow::WorkflowWorkspaceHooks {
		let markdown = format!(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 1
max_retry_backoff_ms = 300000
max_concurrent_agents = 1
gate_profiles = {{}}
canonicalize_commands = []
verify_commands = []

{workspace_hooks_frontmatter}

[context]
read_first = []
+++
			"#,
		);

		WorkflowDocument::parse_markdown(&markdown)
			.expect("workflow should parse")
			.frontmatter()
			.execution()
			.workspace_hooks()
			.clone()
	}

	fn test_git_command() -> Command {
		let mut command = Command::new("git");

		git_credentials::clear_injected_git_config(&mut command);

		command
	}

	fn run_git(repo_root: &Path, args: &[&str]) {
		let output = test_git_command()
			.args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
			.arg("-C")
			.arg(repo_root)
			.args(args)
			.output()
			.expect("git command should run");

		assert!(
			output.status.success(),
			"git {:?} failed in {}: {}",
			args,
			repo_root.display(),
			String::from_utf8_lossy(&output.stderr)
		);
	}

	fn git_stdout(repo_root: &Path, args: &[&str]) -> String {
		let output = test_git_command()
			.arg("-C")
			.arg(repo_root)
			.args(args)
			.output()
			.expect("git command should run");

		assert!(
			output.status.success(),
			"git {:?} failed in {}: {}",
			args,
			repo_root.display(),
			String::from_utf8_lossy(&output.stderr)
		);

		String::from_utf8_lossy(&output.stdout).trim().to_owned()
	}

	fn init_repo() -> (TempDir, PathBuf) {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let repo_root = temp_dir.path().join("repo");
		let default_origin = repo_root.parent().unwrap().join("source-origin.git");

		fs::create_dir_all(&repo_root).expect("repo root should exist");

		run_git(
			default_origin.parent().unwrap(),
			&["init", "--bare", default_origin.to_str().unwrap()],
		);
		run_git(&repo_root, &["init", "--initial-branch", "main"]);
		run_git(&repo_root, &["config", "user.name", "Decodex Tests"]);
		run_git(&repo_root, &["config", "user.email", "decodex-tests@example.com"]);
		run_git(&repo_root, &["config", "commit.gpgsign", "false"]);
		run_git(&repo_root, &["config", "tag.gpgsign", "false"]);
		run_git(&repo_root, &["remote", "add", "origin", default_origin.to_str().unwrap()]);

		fs::write(repo_root.join("README.md"), "hello\n").expect("seed file should write");

		run_git(&repo_root, &["add", "README.md"]);
		run_git(&repo_root, &["commit", "-m", "seed"]);

		(temp_dir, repo_root)
	}

	#[test]
	fn merged_worktree_cleanup_debts_detects_dirty_merged_worktree() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("accounts-column-format");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&[
				"worktree",
				"add",
				"-b",
				"xy/accounts-column-format",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
				"main",
			],
		);

		fs::write(worktree_path.join("README.md"), "feature work\n")
			.expect("worktree file should write");

		run_git(&worktree_path, &["add", "README.md"]);
		run_git(&worktree_path, &["commit", "-m", "feature work"]);
		run_git(
			&repo_root,
			&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
		);

		fs::write(worktree_path.join("README.md"), "dirty after land\n")
			.expect("worktree file should become dirty");

		let debts = super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
			.expect("cleanup debt scan should succeed");

		assert_eq!(debts.len(), 1);
		assert_eq!(debts[0].branch_name, "xy/accounts-column-format");
		assert_eq!(
			fs::canonicalize(&debts[0].path).expect("debt path should canonicalize"),
			fs::canonicalize(&worktree_path).expect("worktree path should canonicalize")
		);
		assert_eq!(debts[0].cleanliness, super::MergedWorktreeCleanliness::Dirty);
	}

	#[test]
	fn merged_worktree_cleanup_debts_treats_decodex_runtime_artifacts_as_clean() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("accounts-column-format");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&[
				"worktree",
				"add",
				"-b",
				"xy/accounts-column-format",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
				"main",
			],
		);

		fs::write(worktree_path.join("README.md"), "feature work\n")
			.expect("worktree file should write");

		run_git(&worktree_path, &["add", "README.md"]);
		run_git(&worktree_path, &["commit", "-m", "feature work"]);
		run_git(
			&repo_root,
			&["merge", "--no-ff", "xy/accounts-column-format", "-m", "land feature"],
		);

		fs::write(worktree_path.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("activity marker should write");

		let control_dir = worktree_path.join(crate::state::RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&control_dir).expect("run-control directory should create");
		fs::write(control_dir.join("run-1-1.channel"), "channel\n")
			.expect("run-control channel should write");

		let debts = super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
			.expect("cleanup debt scan should succeed");

		assert_eq!(debts.len(), 1);
		assert_eq!(debts[0].branch_name, "xy/accounts-column-format");
		assert_eq!(debts[0].cleanliness, super::MergedWorktreeCleanliness::Clean);
	}

	#[test]
	fn merged_worktree_cleanup_debts_ignores_dirty_worktree_started_from_old_default() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("scroll-capture-motion");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&[
				"worktree",
				"add",
				"-b",
				"xy/scroll-capture-motion",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
				"main",
			],
		);

		fs::write(repo_root.join("main.txt"), "main advanced\n")
			.expect("main branch file should write");

		run_git(&repo_root, &["add", "main.txt"]);
		run_git(&repo_root, &["commit", "-m", "advance main"]);

		fs::write(worktree_path.join("README.md"), "manual dirty work\n")
			.expect("worktree file should become dirty");

		let debts = super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
			.expect("cleanup debt scan should succeed");

		assert!(
			debts.is_empty(),
			"dirty worktrees started from an older default commit are manual work, not post-land debt"
		);
	}

	#[test]
	fn merged_worktree_cleanup_debts_ignores_unmerged_worktree() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("dashboard-ws-control-plane");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&[
				"worktree",
				"add",
				"-b",
				"xy/dashboard-ws-control-plane",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
				"main",
			],
		);

		fs::write(worktree_path.join("README.md"), "feature work\n")
			.expect("worktree file should write");

		run_git(&worktree_path, &["add", "README.md"]);
		run_git(&worktree_path, &["commit", "-m", "feature work"]);

		let debts = super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
			.expect("cleanup debt scan should succeed");

		assert!(debts.is_empty(), "unmerged branch worktrees should remain usable");
	}

	#[test]
	fn merged_worktree_cleanup_debts_ignores_dirty_worktree_at_default_tip() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("XY-454");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&[
				"worktree",
				"add",
				"-b",
				"y/decodex-xy-454",
				worktree_path.to_str().expect("worktree path should be UTF-8"),
				"main",
			],
		);

		fs::write(worktree_path.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "started\n")
			.expect("run activity marker should write");

		let debts = super::merged_worktree_cleanup_debts(&repo_root, &worktree_root, "main")
			.expect("cleanup debt scan should succeed");

		assert!(debts.is_empty(), "default-tip run worktrees should remain usable");
	}

	#[test]
	fn plans_worktree_paths_and_identity_scoped_branch_names() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let default_spec = manager.plan_for_issue("PUB-101");

		assert_eq!(default_spec.branch_name, "x/pubfi-pub-101");
		assert_eq!(default_spec.path, worktree_root.join("PUB-101"));
		assert!(!default_spec.reused_existing);

		run_git(&repo_root, &["config", "codex.github-identity", "y"]);

		let routed_spec = manager.plan_for_issue("PUB-101");

		assert_eq!(routed_spec.branch_name, "y/pubfi-pub-101");
	}

	#[test]
	fn workspace_hook_shell_uses_posix_sh_for_sh_or_missing_shell() {
		for shell_env in [Some(std::ffi::OsString::from("/bin/sh")), None] {
			let (shell, shell_flag) = super::workspace_hook_shell_from_env(shell_env);

			assert_eq!(shell, std::ffi::OsString::from("/bin/sh"));
			assert_eq!(shell_flag, "-c");
		}
	}

	#[test]
	fn creates_linked_worktree() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		assert_eq!(spec.branch_name, "x/pubfi-pub-101");
		assert!(spec.path.join(".git").is_file());
		assert_eq!(
			git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]),
			"x/pubfi-pub-101"
		);

		let repo_git_dir = fs::canonicalize(repo_root.join(".git")).expect("repo git dir");
		let git_dir = fs::canonicalize(PathBuf::from(git_stdout(
			&spec.path,
			&["rev-parse", "--path-format=absolute", "--git-dir"],
		)))
		.expect("git dir should canonicalize");
		let git_common_dir = fs::canonicalize(PathBuf::from(git_stdout(
			&spec.path,
			&["rev-parse", "--path-format=absolute", "--git-common-dir"],
		)))
		.expect("git common dir should canonicalize");

		assert!(git_dir.starts_with(repo_git_dir.join("worktrees")));
		assert_eq!(git_common_dir, repo_git_dir);
		assert!(
			super::worktree_is_registered(
				&repo_root,
				&fs::canonicalize(&spec.path).expect("canonical worktree path")
			)
			.expect("worktree registration should inspect")
		);
	}

	#[test]
	fn after_create_hook_runs_only_for_new_worktree() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let hook_log = repo_root.join("after-create.log");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" >> \"$DECODEX_REPO_ROOT/after-create.log\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let created = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("worktree should be created");
		let reused = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("worktree should be reused");

		assert!(!created.reused_existing);
		assert!(reused.reused_existing);
		assert_eq!(
			fs::read_to_string(&hook_log).expect("hook log should exist"),
			"x/pubfi-pub-101\n"
		);
	}

	#[test]
	fn after_create_hook_failure_keeps_created_worktree() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["exit 23"]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let planned = manager.plan_for_issue("PUB-101");
		let error = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect_err("after_create hook failure should stop setup");

		assert!(
			error.to_string().contains("Workspace hook `after_create` command `exit 23` failed")
		);
		assert!(planned.path.exists(), "failed hook should keep the worktree for inspection");
		assert!(planned.path.join(".git").is_file(), "failed hook should keep the linked worktree");
		assert!(
			super::after_create_pending_marker_path(&planned.path).exists(),
			"failed after-create hook should leave a pending bootstrap marker"
		);
	}

	#[test]
	fn reused_lane_retries_bootstrap_after_interrupted_create_window() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let hook_log = repo_root.join("after-create.log");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" >> \"$DECODEX_REPO_ROOT/after-create.log\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let planned = manager.plan_for_issue("PUB-101");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		manager
			.create_linked_worktree(&planned, Some(&hooks))
			.expect("linked worktree should be created");
		manager
			.validate_worktree_boundary(&planned.path)
			.expect("created worktree should validate");

		assert!(
			super::after_create_pending_marker_path(&planned.path).exists(),
			"newly created lane should persist the pending bootstrap marker before first hook run"
		);
		assert!(!hook_log.exists(), "simulated crash window should not have run hooks yet");

		let reused = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("reused lane should resume interrupted bootstrap");

		assert!(reused.reused_existing);
		assert_eq!(
			fs::read_to_string(&hook_log).expect("hook log should exist after resumed bootstrap"),
			"x/pubfi-pub-101\n"
		);
		assert!(
			!super::after_create_pending_marker_path(&planned.path).exists(),
			"successful resumed bootstrap should clear the pending marker"
		);
	}

	#[test]
	fn after_create_hook_retries_before_reused_lane_dispatch() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let hook_log = repo_root.join("after-create.log");
		let allow_file = repo_root.join("allow-bootstrap");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = ["printf '%s\n' \"$DECODEX_BRANCH\" >> \"$DECODEX_REPO_ROOT/after-create.log\" && test -f \"$DECODEX_REPO_ROOT/allow-bootstrap\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
		);
		let planned = manager.plan_for_issue("PUB-101");
		let first_error = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect_err("missing bootstrap prerequisite should fail");

		assert!(first_error.to_string().contains("Workspace hook `after_create` command"));
		assert_eq!(
			fs::read_to_string(&hook_log).expect("hook log should exist after first failure"),
			"x/pubfi-pub-101\n"
		);
		assert!(
			super::after_create_pending_marker_path(&planned.path).exists(),
			"failed bootstrap should leave the pending marker behind"
		);

		fs::write(&allow_file, "ready\n").expect("bootstrap prerequisite should write");

		let reused = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("reused lane should rerun the pending bootstrap hook");

		assert!(reused.reused_existing);
		assert_eq!(
			fs::read_to_string(&hook_log).expect("hook log should include retried bootstrap"),
			"x/pubfi-pub-101\nx/pubfi-pub-101\n"
		);
		assert!(
			!super::after_create_pending_marker_path(&planned.path).exists(),
			"successful retry should clear the pending bootstrap marker"
		);
	}

	#[test]
	fn after_create_hook_handles_hook_managed_pending_marker_removal() {
		{
			let (_temp_dir, repo_root) = init_repo();
			let worktree_root = repo_root.join(".worktrees");
			let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
			let hooks = workspace_hooks(
				r#"
[execution.workspace_hooks]
after_create_commands = ["rm -f \"$DECODEX_WORKTREE_PATH/.decodex-after-create.pending\""]
before_remove_commands = []
timeout_seconds = 60
			"#,
			);
			let spec = manager
				.ensure_worktree_with_hooks("PUB-101", false, &hooks)
				.expect("successful hook that removes the marker should still pass");

			assert!(spec.path.exists(), "worktree should remain usable after bootstrap");
			assert!(
				!super::after_create_pending_marker_path(&spec.path).exists(),
				"successful hook should not leave a stale pending marker behind"
			);
		}
		{
			let (_temp_dir, repo_root) = init_repo();
			let worktree_root = repo_root.join(".worktrees");
			let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
			let hooks = workspace_hooks(
				r#"
[execution.workspace_hooks]
after_create_commands = ["rm -f \"$DECODEX_WORKTREE_PATH/.decodex-after-create.pending\"", "exit 23"]
before_remove_commands = []
timeout_seconds = 60
			"#,
			);
			let planned = manager.plan_for_issue("PUB-101");
			let error = manager
				.ensure_worktree_with_hooks("PUB-101", false, &hooks)
				.expect_err("failed hook should still leave the lane pending for retry");

			assert!(
				error
					.to_string()
					.contains("Workspace hook `after_create` command `exit 23` failed")
			);
			assert!(
				super::after_create_pending_marker_path(&planned.path).exists(),
				"failed bootstrap should restore the pending marker even if an earlier command removed it"
			);
		}
	}

	#[test]
	fn workspace_hook_command_returns_without_waiting_for_background_child_pipe_close() {
		let (_temp_dir, repo_root) = init_repo();
		let start = Instant::now();
		let output = super::run_workspace_hook_shell_command(
			"sleep 5 & printf 'done\\n'",
			&repo_root,
			&[],
			Duration::from_secs(1),
		)
		.expect("shell exit should not block on inherited stdout/stderr pipe handles");

		assert!(output.status.success(), "backgrounded child should not fail the shell command");
		assert_eq!(String::from_utf8_lossy(&output.stdout), "done\n");
		assert!(
			start.elapsed() < Duration::from_secs(3),
			"hook output collection should not wait for background child pipe closure after shell exit"
		);
	}

	#[cfg(unix)]
	#[test]
	fn workspace_hook_timeout_kills_background_descendants() {
		let (_temp_dir, repo_root) = init_repo();
		let child_pid_file = repo_root.join("hook-child.pid");
		let error = super::run_workspace_hook_shell_command(
			"sleep 300 & bg=$!; printf '%s\n' \"$bg\" > \"$DECODEX_REPO_ROOT/hook-child.pid\"; wait",
			&repo_root,
			&[("DECODEX_REPO_ROOT", repo_root.display().to_string())],
			Duration::from_secs(1),
		)
		.expect_err("timed out hook should fail");

		assert!(error.to_string().contains("exceeded the 1s timeout"));

		let child_pid = fs::read_to_string(&child_pid_file)
			.expect("background child pid should be recorded before timeout")
			.trim()
			.parse::<i32>()
			.expect("background child pid should parse");
		let kill_deadline = Instant::now() + Duration::from_secs(2);

		while process_is_alive(child_pid) && Instant::now() < kill_deadline {
			thread::sleep(Duration::from_millis(25));
		}

		assert!(
			!process_is_alive(child_pid),
			"timed out workspace hook should terminate background descendants"
		);
	}

	#[cfg(unix)]
	fn process_is_alive(process_id: i32) -> bool {
		let result = unsafe { libc::kill(process_id, 0) };

		if result == 0 {
			return true;
		}

		std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
	}

	#[test]
	fn after_create_hook_tolerates_verbose_success_output() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
timeout_seconds = 1
after_create_commands = ["yes hook-output | head -c 131072 >/dev/stdout"]
before_remove_commands = []
			"#,
		);
		let spec = manager
			.ensure_worktree_with_hooks("PUB-101", false, &hooks)
			.expect("verbose successful hook should not deadlock on captured output");

		assert!(spec.path.exists(), "worktree should remain usable after verbose bootstrap");
		assert!(
			!super::after_create_pending_marker_path(&spec.path).exists(),
			"successful verbose hook should clear the pending marker"
		);
	}

	#[test]
	fn creates_linked_worktree_when_repo_root_is_also_a_linked_worktree() {
		let (_temp_dir, primary_repo_root) = init_repo();
		let linked_repo_root = primary_repo_root.parent().unwrap().join("linked-root");

		run_git(
			&primary_repo_root,
			&["worktree", "add", "--quiet", "--detach", linked_repo_root.to_str().unwrap(), "HEAD"],
		);
		run_git(&linked_repo_root, &["checkout", "--quiet", "-B", "x/pubfi-linked-root", "HEAD"]);

		let worktree_root = linked_repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &linked_repo_root, &worktree_root);
		let spec = manager
			.ensure_worktree("PUB-101", false)
			.expect("worktree should be created from linked repo root");

		assert_eq!(spec.branch_name, "x/pubfi-pub-101");
		assert!(spec.path.join(".git").is_file());

		let repo_git_dir = fs::canonicalize(PathBuf::from(git_stdout(
			&linked_repo_root,
			&["rev-parse", "--path-format=absolute", "--git-common-dir"],
		)))
		.expect("linked repo common dir should canonicalize");
		let git_dir = fs::canonicalize(PathBuf::from(git_stdout(
			&spec.path,
			&["rev-parse", "--path-format=absolute", "--git-dir"],
		)))
		.expect("git dir should canonicalize");
		let git_common_dir = fs::canonicalize(PathBuf::from(git_stdout(
			&spec.path,
			&["rev-parse", "--path-format=absolute", "--git-common-dir"],
		)))
		.expect("git common dir should canonicalize");

		assert!(git_dir.starts_with(repo_git_dir.join("worktrees")));
		assert_eq!(git_common_dir, repo_git_dir);
	}

	#[test]
	fn linked_worktree_inherits_repo_local_identity_config() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

		run_git(&repo_root, &["config", "user.signingkey", "worktree-tests"]);
		run_git(&repo_root, &["config", "codex.github-identity", "y"]);
		run_git(&repo_root, &["config", "codex.linear-workspace", "hackink"]);

		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		assert_eq!(git_stdout(&spec.path, &["config", "--get", "user.name"]), "Decodex Tests");
		assert_eq!(
			git_stdout(&spec.path, &["config", "--get", "user.email"]),
			"decodex-tests@example.com"
		);
		assert_eq!(git_stdout(&spec.path, &["config", "--get", "commit.gpgsign"]), "false");
		assert_eq!(
			git_stdout(&spec.path, &["config", "--get", "user.signingkey"]),
			"worktree-tests"
		);
		assert_eq!(git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
		assert_eq!(
			git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
			"hackink"
		);
	}

	#[test]
	fn linked_worktree_inherits_repo_local_identity_from_included_config() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let included_config = repo_root.parent().unwrap().join("identity.inc");

		run_git(&repo_root, &["config", "--unset-all", "user.name"]);
		run_git(&repo_root, &["config", "--unset-all", "user.email"]);

		fs::write(
			&included_config,
			"[user]\n\tname = Included Tests\n\temail = included@example.com\n[codex]\n\tgithub-identity = y\n\tlinear-workspace = hackink\n",
		)
		.expect("included config should write");

		run_git(
			&repo_root,
			&["config", "--local", "include.path", included_config.to_str().unwrap()],
		);

		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		assert_eq!(git_stdout(&spec.path, &["config", "--get", "user.name"]), "Included Tests");
		assert_eq!(
			git_stdout(&spec.path, &["config", "--get", "user.email"]),
			"included@example.com"
		);
		assert_eq!(git_stdout(&spec.path, &["config", "--get", "codex.github-identity"]), "y");
		assert_eq!(
			git_stdout(&spec.path, &["config", "--get", "codex.linear-workspace"]),
			"hackink"
		);
	}

	#[test]
	fn linked_worktree_uses_existing_remote_lane_branch_when_present() {
		let (_temp_dir, repo_root) = init_repo();
		let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let lane_branch = "x/pubfi-pub-101";

		run_git(bare_remote.parent().unwrap(), &["init", "--bare", bare_remote.to_str().unwrap()]);
		run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
		run_git(&repo_root, &["push", "-u", "origin", "main"]);
		run_git(&repo_root, &["checkout", "-b", lane_branch]);

		fs::write(repo_root.join("LANE.md"), "lane branch\n").expect("lane file should write");

		run_git(&repo_root, &["add", "LANE.md"]);
		run_git(&repo_root, &["commit", "-m", "lane branch"]);
		run_git(&repo_root, &["push", "-u", "origin", lane_branch]);
		run_git(&repo_root, &["checkout", "main"]);

		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		assert_eq!(git_stdout(&spec.path, &["rev-parse", "--abbrev-ref", "HEAD"]), lane_branch);
		assert_eq!(
			fs::read_to_string(spec.path.join("LANE.md")).expect("lane file should exist"),
			"lane branch\n"
		);
		assert_eq!(
			git_stdout(&spec.path, &["remote", "get-url", "origin"]),
			fs::canonicalize(&bare_remote)
				.expect("bare remote should canonicalize")
				.to_str()
				.expect("bare remote should be valid UTF-8")
		);
	}

	#[test]
	fn linked_worktree_push_uses_normalized_absolute_origin_when_source_remote_is_relative() {
		let (_temp_dir, repo_root) = init_repo();
		let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

		run_git(bare_remote.parent().unwrap(), &["init", "--bare", bare_remote.to_str().unwrap()]);
		run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
		run_git(&repo_root, &["push", "-u", "origin", "main"]);

		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		fs::write(spec.path.join("WORKTREE.md"), "linked worktree lane\n")
			.expect("worktree file should write");

		run_git(&spec.path, &["add", "WORKTREE.md"]);
		run_git(&spec.path, &["commit", "-m", "worktree change"]);
		run_git(&spec.path, &["push", "-u", "origin", "x/pubfi-pub-101"]);

		assert_eq!(
			git_stdout(&spec.path, &["remote", "get-url", "origin"]),
			fs::canonicalize(&bare_remote)
				.expect("bare remote should canonicalize")
				.to_str()
				.expect("bare remote should be valid UTF-8")
		);
	}

	#[test]
	fn reused_linked_worktree_normalizes_relative_origin_on_reentry() {
		let (_temp_dir, repo_root) = init_repo();
		let bare_remote = repo_root.parent().unwrap().join("lane-remote.git");
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);

		run_git(bare_remote.parent().unwrap(), &["init", "--bare", bare_remote.to_str().unwrap()]);
		run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);
		run_git(&repo_root, &["push", "-u", "origin", "main"]);

		let created =
			manager.ensure_worktree("PUB-101", false).expect("worktree should be created");

		run_git(&repo_root, &["remote", "set-url", "origin", "../lane-remote.git"]);

		let reused = manager.ensure_worktree("PUB-101", false).expect("worktree should be reused");

		assert!(reused.reused_existing);
		assert_eq!(reused.path, created.path);
		assert_eq!(
			git_stdout(&reused.path, &["remote", "get-url", "origin"]),
			fs::canonicalize(&bare_remote)
				.expect("bare remote should canonicalize")
				.to_str()
				.expect("bare remote should be valid UTF-8")
		);
	}

	#[test]
	fn linked_worktree_leaves_home_relative_origin_unchanged() {
		let (_temp_dir, repo_root) = init_repo();

		run_git(&repo_root, &["remote", "set-url", "origin", "~/lane-remote.git"]);

		super::normalize_origin_remote_for_worktrees(&repo_root)
			.expect("home-relative remotes should bypass normalization");

		assert_eq!(git_stdout(&repo_root, &["remote", "get-url", "origin"]), "~/lane-remote.git");
		assert!(!super::is_relative_filesystem_remote("~/lane-remote.git"));
		assert!(!super::is_relative_filesystem_remote("~"));
	}

	#[test]
	fn linked_worktree_rolls_back_when_origin_normalization_fails() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let spec = manager.plan_for_issue("PUB-101");

		run_git(&repo_root, &["remote", "set-url", "origin", "../missing-remote.git"]);

		let error = manager
			.ensure_worktree("PUB-101", false)
			.expect_err("worktree creation should fail when origin normalization fails");

		assert!(
			error.to_string().contains("No such file or directory")
				|| error.to_string().contains("does not exist"),
			"unexpected error: {error:?}"
		);
		assert!(!spec.path.exists(), "failed setup should remove the new worktree path");
		assert!(
			!super::worktree_is_registered(&repo_root, &spec.path)
				.expect("worktree registration should inspect"),
			"failed setup should unregister the new worktree"
		);
	}

	#[test]
	fn linked_worktree_fails_when_remote_branch_probe_errors() {
		let (temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let missing_remote = temp_dir.path().join("missing-origin.git");

		run_git(&repo_root, &["remote", "set-url", "origin", missing_remote.to_str().unwrap()]);

		let error = manager
			.ensure_worktree("PUB-101", false)
			.expect_err("worktree create should fail when remote probe errors");

		assert!(error.to_string().contains("Failed to inspect remote worktree branch"));
	}

	#[test]
	fn rejects_reused_non_worktree_checkout_with_embedded_git_dir() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let worktree_path = worktree_root.join("PUB-101");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");

		run_git(
			&repo_root,
			&["clone", "--quiet", "--no-checkout", ".", worktree_path.to_str().unwrap()],
		);

		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let error = manager
			.ensure_worktree("PUB-101", false)
			.expect_err("embedded git checkout should be rejected");

		assert!(
			error
				.to_string()
				.contains("is not a linked git worktree: expected `.git` to be a pointer file")
		);
	}

	#[test]
	fn removes_linked_worktree_path() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");

		assert!(manager.remove_worktree_path(&spec.path).expect("worktree should remove"));
		assert!(!spec.path.exists());
		assert!(
			!git_stdout(&repo_root, &["worktree", "list", "--porcelain"])
				.contains(&format!("worktree {}", spec.path.display()))
		);
	}

	#[test]
	fn removes_orphaned_marker_directory_without_linked_git_metadata() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let orphan_path = worktree_root.join("PUB-101");
		let hook_log = repo_root.join("before-remove.log");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf 'hook-ran\n' > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
		);

		fs::create_dir_all(&orphan_path).expect("orphan path should exist");
		fs::write(orphan_path.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "run_id=run-orphan\n")
			.expect("runtime marker should write");

		assert!(
			manager
				.remove_worktree_path_with_hooks("PUB-101", "x/pubfi-pub-101", &orphan_path, &hooks,)
				.expect("orphan marker directory should remove")
		);
		assert!(!orphan_path.exists(), "orphan marker directory should be deleted");
		assert!(
			!hook_log.exists(),
			"before_remove hook should not run for a non-worktree marker directory"
		);
	}

	#[test]
	fn before_remove_hook_runs_before_cleanup() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let hook_log = repo_root.join("before-remove.log");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf '%s:%s\n' \"$DECODEX_ISSUE_ID\" \"$DECODEX_BRANCH\" > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
		);
		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");

		assert!(
			manager
				.remove_worktree_path_with_hooks(
					&spec.issue_identifier,
					&spec.branch_name,
					&spec.path,
					&hooks
				)
				.expect("worktree should remove")
		);
		assert_eq!(
			fs::read_to_string(&hook_log).expect("hook log should exist"),
			"PUB-101:x/pubfi-pub-101\n"
		);
		assert!(!spec.path.exists(), "successful cleanup should still remove the worktree");
	}

	#[test]
	fn before_remove_hook_failure_blocks_cleanup() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["exit 19"]
timeout_seconds = 60
			"#,
		);
		let spec = manager.ensure_worktree("PUB-101", false).expect("worktree should exist");
		let error = manager
			.remove_worktree_path_with_hooks(
				&spec.issue_identifier,
				&spec.branch_name,
				&spec.path,
				&hooks,
			)
			.expect_err("before_remove hook failure should block cleanup");

		assert!(
			error.to_string().contains("Workspace hook `before_remove` command `exit 19` failed")
		);
		assert!(spec.path.exists(), "blocked cleanup should keep the worktree");
	}

	#[test]
	fn before_remove_hook_does_not_run_for_unregistered_directory() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let rogue_path = worktree_root.join("PUB-rogue");
		let hook_log = repo_root.join("before-remove.log");

		fs::create_dir_all(&rogue_path).expect("rogue path should exist");
		fs::write(rogue_path.join(".git"), b"not-a-worktree\n")
			.expect("rogue path should contain a fake git pointer");

		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let hooks = workspace_hooks(
			r#"
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = ["printf 'hook-ran\n' > \"$DECODEX_REPO_ROOT/before-remove.log\""]
timeout_seconds = 60
			"#,
		);
		let error = manager
			.remove_worktree_path_with_hooks("PUB-rogue", "x/pubfi-pub-rogue", &rogue_path, &hooks)
			.expect_err("unregistered directory should fail validation before before_remove hooks");

		assert!(
			!error.to_string().trim().is_empty(),
			"validation failure should still surface an actionable error"
		);
		assert!(
			!hook_log.exists(),
			"before_remove hook should not run before linked worktree validation succeeds"
		);
		assert!(
			rogue_path.exists(),
			"failed validation should leave the unregistered directory untouched"
		);
	}

	#[test]
	fn rejects_worktree_removal_when_path_escapes_root_via_parent_components() {
		let (_temp_dir, repo_root) = init_repo();
		let worktree_root = repo_root.join(".worktrees");
		let escaped_target = repo_root.join("outside").join("PUB-101");

		fs::create_dir_all(&worktree_root).expect("worktree root should exist");
		fs::create_dir_all(&escaped_target).expect("escaped target should exist");

		let manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
		let escaped_path = worktree_root.join("../outside/PUB-101");
		let error = manager
			.remove_worktree_path(&escaped_path)
			.expect_err("escaped worktree path should be rejected");

		assert!(error.to_string().contains("outside worktree_root"));
		assert!(escaped_target.exists());
	}
}
