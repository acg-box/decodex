use std::{
	env,
	ffi::OsString,
	path::PathBuf,
	process::{Command, Stdio},
};

use crate::{
	agent::json_rpc::environment::{
		codex_home::ResolvedAppServerCodexHomeEnv, process_env::AppServerProcessEnv,
	},
	prelude::Result,
};

pub(crate) const APP_SERVER_STDERR_TAIL_LINES: usize = 20;

const CODEX_APP_SERVER_BINARY: &str = "codex";

pub(crate) fn app_server_command_program() -> PathBuf {
	app_server_command_program_from_env(env::var_os("PATH"), env::var_os("HOME"))
}

pub(crate) fn configure_app_server_command(
	command: &mut Command,
	listen: &str,
	process_env: &AppServerProcessEnv,
) -> Result<ResolvedAppServerCodexHomeEnv> {
	command
		.args(["app-server", "--listen", listen])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());

	process_env.apply_to(command)
}

pub(crate) fn app_server_command_program_from_env(
	path_env: Option<OsString>,
	home: Option<OsString>,
) -> PathBuf {
	if let Some(path_env) = path_env {
		for path_entry in env::split_paths(&path_env) {
			let candidate = path_entry.join(CODEX_APP_SERVER_BINARY);

			if candidate.is_file() {
				return candidate;
			}
		}
	}
	if let Some(home) = home {
		let home = PathBuf::from(home);

		for relative_candidate in
			[[".local", "bin", CODEX_APP_SERVER_BINARY], [".cargo", "bin", CODEX_APP_SERVER_BINARY]]
		{
			let candidate = relative_candidate
				.iter()
				.fold(home.clone(), |path, component| path.join(*component));

			if candidate.is_file() {
				return candidate;
			}
		}
	}

	PathBuf::from(CODEX_APP_SERVER_BINARY)
}
