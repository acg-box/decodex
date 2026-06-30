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
	state,
	workflow::WorkflowWorkspaceHooks,
};

mod cleanup;

#[allow(unused_imports)] pub(crate) use cleanup::MergedWorktreeCleanliness;
pub(crate) use cleanup::{
	MergedWorktreeCleanupDebt, infer_default_branch_name, merged_worktree_cleanup_debts,
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
		if !path.try_exists().map_err(|error| {
			eyre::eyre!("Failed to inspect worktree path `{}`: {error}", path.display())
		})? {
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

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let entry_path = entry.path();
		let relative = entry_path.strip_prefix(path).unwrap_or(entry_path.as_path());

		if relative == Path::new(AFTER_CREATE_PENDING_MARKER)
			|| state::is_decodex_runtime_artifact_relative_path(relative)
		{
			continue;
		}

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

#[cfg(test)] mod tests;
