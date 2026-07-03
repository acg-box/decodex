use super::super::{
	constants::{
		PROBE_COMMAND_EXEC_EXPECTED_OUTPUT, PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
		PROBE_COMMAND_EXEC_TIMEOUT_MS,
	},
	protocol::{AppServerClient, CommandExecParams, CommandExecResponse},
	runtime_types::{AppServerRunRequest, RunRecorder},
	turn_loop::flush_pending_messages,
};
use crate::prelude::eyre;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandExecHealthCheck {
	pub(crate) command: Vec<String>,
	pub(crate) expected_stdout: String,
	pub(crate) timeout_ms: u64,
	pub(crate) output_bytes_cap: u64,
}
impl CommandExecHealthCheck {
	pub(in crate::agent::app_server) fn probe() -> Self {
		Self {
			command: vec![
				String::from("/bin/sh"),
				String::from("-c"),
				format!("printf {PROBE_COMMAND_EXEC_EXPECTED_OUTPUT}"),
			],
			expected_stdout: String::from(PROBE_COMMAND_EXEC_EXPECTED_OUTPUT),
			timeout_ms: PROBE_COMMAND_EXEC_TIMEOUT_MS,
			output_bytes_cap: PROBE_COMMAND_EXEC_OUTPUT_BYTES_CAP,
		}
	}
}

pub(in crate::agent::app_server) fn run_command_exec_health_check(
	client: &mut AppServerClient,
	recorder: &mut RunRecorder<'_>,
	request: &AppServerRunRequest<'_>,
	health_check: &CommandExecHealthCheck,
) -> crate::prelude::Result<()> {
	let params = build_command_exec_health_check_params(health_check, &request.cwd);
	let response = client.command_exec(&params)?;

	flush_pending_messages(client, recorder, None)?;

	validate_command_exec_health_check_result(health_check, &response)
}

pub(in crate::agent::app_server) fn build_command_exec_health_check_params(
	health_check: &CommandExecHealthCheck,
	cwd: &str,
) -> CommandExecParams {
	CommandExecParams {
		command: health_check.command.clone(),
		cwd: Some(cwd.to_owned()),
		timeout_ms: Some(health_check.timeout_ms),
		output_bytes_cap: Some(health_check.output_bytes_cap),
	}
}

pub(in crate::agent::app_server) fn validate_command_exec_health_check_result(
	health_check: &CommandExecHealthCheck,
	response: &CommandExecResponse,
) -> crate::prelude::Result<()> {
	if response.exit_code != 0 {
		eyre::bail!(
			"`command/exec` health check failed with exit code {}. stdout: {:?}; stderr: {:?}",
			response.exit_code,
			response.stdout,
			response.stderr
		);
	}
	if response.stdout != health_check.expected_stdout {
		eyre::bail!(
			"`command/exec` health check returned stdout {:?}, expected {:?}. stderr: {:?}",
			response.stdout,
			health_check.expected_stdout,
			response.stderr
		);
	}
	if !response.stderr.is_empty() {
		eyre::bail!("`command/exec` health check wrote unexpected stderr: {:?}", response.stderr);
	}

	Ok(())
}
