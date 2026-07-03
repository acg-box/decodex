use std::path::{Path, PathBuf};

use color_eyre::Report;

use crate::{
	agent::{
		app_server::protocol::{EffectiveThreadConfig, InitializeResponse},
		json_rpc::{AppServerHomePreflightFailure, ResolvedAppServerCodexHomeEnv},
	},
	prelude::{Result, eyre},
};

pub(in crate::agent::app_server) fn validate_effective_thread_config(
	cwd: &str,
	runtime: &EffectiveThreadConfig,
) -> Result<()> {
	if runtime.cwd != cwd {
		eyre::bail!(
			"app_server_protocol_failure: effective cwd `{}` did not match requested worktree `{cwd}`.",
			runtime.cwd
		);
	}
	if runtime.approval_policy != "never" {
		eyre::bail!(
			"app_server_protocol_failure: effective approval policy `{}` is interactive; Decodex requires `never`.",
			runtime.approval_policy
		);
	}
	if runtime.sandbox_mode == "readOnly" {
		eyre::bail!(
			"app_server_protocol_failure: effective sandbox mode `readOnly` does not allow Decodex execution."
		);
	}

	Ok(())
}

pub(in crate::agent::app_server) fn validate_initialize_codex_home(
	expected: &ResolvedAppServerCodexHomeEnv,
	response: &InitializeResponse,
) -> Result<()> {
	let expected_home = normalized_home_path(expected.codex_home());
	let resolved_home = normalized_home_path(Path::new(&response.codex_home));

	if resolved_home != expected_home {
		tracing::warn!(
			expected_codex_home = %expected.codex_home().display(),
			resolved_codex_home = %response.codex_home,
			"Codex app-server resolved an unexpected Codex home."
		);

		return Err(Report::new(AppServerHomePreflightFailure::initialize_mismatch(
			response.codex_home.clone(),
			expected.codex_home().display().to_string(),
		)));
	}

	Ok(())
}

pub(in crate::agent::app_server) fn thread_resume_error_allows_fallback(error: &Report) -> bool {
	let message = error.to_string().to_lowercase();

	thread_missing_error_message_allows_discard(&message)
}

pub(in crate::agent::app_server) fn thread_missing_error_message_allows_discard(
	message: &str,
) -> bool {
	message.contains("no rollout found for thread id") || message.contains("thread not found")
}

fn normalized_home_path(path: &Path) -> PathBuf {
	path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
