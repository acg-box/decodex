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

#[cfg(unix)] use std::os::{fd::AsRawFd, unix::process::CommandExt as _};

use libc::{ESRCH, F_GETFL, F_SETFL, O_NONBLOCK, SIGKILL};

use crate::{
	prelude::{Result, eyre},
	state,
	workflow::WorkflowWorkspaceHooks,
};

const AFTER_CREATE_PENDING_MARKER: &str = ".decodex-after-create.pending";
const WORKSPACE_HOOK_CAPTURE_LIMIT: usize = 1_024 * 1_024;
const WORKSPACE_HOOK_TRUNCATED_MARKER: &[u8] = b"\n[decodex truncated workspace hook output]\n";

pub(super) fn after_create_pending_marker_path(worktree_path: &Path) -> PathBuf {
	worktree_path.join(AFTER_CREATE_PENDING_MARKER)
}

pub(super) fn workspace_requires_after_create_pending_marker(
	hooks: Option<&WorkflowWorkspaceHooks>,
) -> bool {
	hooks.is_some_and(|hooks| !hooks.after_create_commands().is_empty())
}

pub(super) fn remove_orphan_marker_directory_if_safe(path: &Path) -> Result<bool> {
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

pub(super) fn workspace_hook_shell_from_env(
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
pub(super) fn run_workspace_hook_shell_command(
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

pub(super) fn append_output_details(buffer: &mut String, output: &Output) {
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
