use std::{
	env, fs,
	io::{self, Read, Write as _},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
	thread::{self, JoinHandle},
	time::Duration,
};

use time::OffsetDateTime;

use crate::prelude::{Result, eyre};

use super::{AccountListResponse, AccountLoginRequest, AccountStore, secure_account_file};

enum LoginPipeEvent {
	Chunk(Vec<u8>),
	ReaderFailed(String),
}

pub(crate) fn run_account_login(request: &AccountLoginRequest) -> Result<()> {
	let response = account_login(request, |chunk| {
		print!("{chunk}");

		io::stdout().flush()?;

		Ok(())
	})?;

	super::print_list_response(&response, false)
}

pub(crate) fn account_login(
	request: &AccountLoginRequest,
	on_output: impl FnMut(&str) -> Result<()>,
) -> Result<AccountListResponse> {
	let temp_home = create_login_home()?;
	let status = run_codex_device_login(&request.codex_bin, &temp_home, on_output)?;

	if !status.success() {
		cleanup_login_home(&temp_home, request.keep_temp_home);

		eyre::bail!("Codex account login failed with status {status}.");
	}

	let auth_json_path = temp_home.join("auth.json");
	let store = AccountStore::global()?;
	let import_result = store.import_auth_json(&auth_json_path);

	cleanup_login_home(&temp_home, request.keep_temp_home);

	import_result
}

fn run_codex_device_login(
	codex_bin: &str,
	temp_home: &Path,
	on_output: impl FnMut(&str) -> Result<()>,
) -> Result<ExitStatus> {
	let mut child = Command::new(codex_bin)
		.arg("login")
		.arg("--device-auth")
		.env("CODEX_HOME", temp_home)
		.env("CODEX_SQLITE_HOME", temp_home)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|error| {
			eyre::eyre!("Failed to start `{codex_bin}` for Codex account login: {error}")
		})?;
	let stdout =
		child.stdout.take().ok_or_else(|| eyre::eyre!("Failed to capture Codex login stdout."))?;
	let stderr =
		child.stderr.take().ok_or_else(|| eyre::eyre!("Failed to capture Codex login stderr."))?;
	let (sender, receiver) = mpsc::channel();
	let stdout_reader = spawn_login_pipe_reader(stdout, sender.clone());
	let stderr_reader = spawn_login_pipe_reader(stderr, sender);
	let status = wait_for_login_child(child, receiver, on_output)?;

	join_login_pipe_reader(stdout_reader)?;
	join_login_pipe_reader(stderr_reader)?;

	Ok(status)
}

fn spawn_login_pipe_reader(
	mut reader: impl Read + Send + 'static,
	sender: Sender<LoginPipeEvent>,
) -> JoinHandle<()> {
	thread::spawn(move || {
		let mut buffer = [0_u8; 4_096];

		loop {
			match reader.read(&mut buffer) {
				Ok(0) => return,
				Ok(len) => {
					if sender.send(LoginPipeEvent::Chunk(buffer[..len].to_vec())).is_err() {
						return;
					}
				},
				Err(error) => {
					let _ = sender.send(LoginPipeEvent::ReaderFailed(error.to_string()));

					return;
				},
			}
		}
	})
}

fn wait_for_login_child(
	mut child: Child,
	receiver: Receiver<LoginPipeEvent>,
	mut on_output: impl FnMut(&str) -> Result<()>,
) -> Result<ExitStatus> {
	let mut reader_error = None;

	loop {
		while let Ok(event) = receiver.try_recv() {
			handle_login_pipe_event(event, &mut on_output, &mut reader_error)?;
		}

		if let Some(status) = child.try_wait()? {
			while let Ok(event) = receiver.try_recv() {
				handle_login_pipe_event(event, &mut on_output, &mut reader_error)?;
			}

			if let Some(error) = reader_error {
				eyre::bail!("Failed while reading Codex login output: {error}");
			}

			return Ok(status);
		}

		match receiver.recv_timeout(Duration::from_millis(50)) {
			Ok(event) => handle_login_pipe_event(event, &mut on_output, &mut reader_error)?,
			Err(RecvTimeoutError::Timeout) => {},
			Err(RecvTimeoutError::Disconnected) => {
				let status = child.wait()?;

				if let Some(error) = reader_error {
					eyre::bail!("Failed while reading Codex login output: {error}");
				}

				return Ok(status);
			},
		}
	}
}

fn handle_login_pipe_event(
	event: LoginPipeEvent,
	on_output: &mut impl FnMut(&str) -> Result<()>,
	reader_error: &mut Option<String>,
) -> Result<()> {
	match event {
		LoginPipeEvent::Chunk(chunk) => on_output(&String::from_utf8_lossy(&chunk))?,
		LoginPipeEvent::ReaderFailed(error) => *reader_error = Some(error),
	}

	Ok(())
}

fn join_login_pipe_reader(handle: JoinHandle<()>) -> Result<()> {
	handle.join().map_err(|_| eyre::eyre!("Codex login output reader panicked."))
}

fn create_login_home() -> Result<PathBuf> {
	let root = env::temp_dir().join(format!(
		"decodex-codex-login-{}-{}",
		std::process::id(),
		OffsetDateTime::now_utc().unix_timestamp()
	));

	fs::create_dir_all(&root)?;

	secure_account_file(&root)?;

	Ok(root)
}

fn cleanup_login_home(path: &Path, keep: bool) {
	if keep {
		eprintln!("temporary Codex login home preserved at {}", path.display());

		return;
	}

	if let Err(error) = fs::remove_dir_all(path) {
		eprintln!(
			"warning: failed to remove temporary Codex login home `{}`: {error}",
			path.display()
		);
	}
}
