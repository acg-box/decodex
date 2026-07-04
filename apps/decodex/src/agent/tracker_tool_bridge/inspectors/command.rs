use std::{path::Path, process::Command};

pub(in crate::agent::tracker_tool_bridge) fn run_command_for_stdout(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	let stdout = run_command_stdout(command, args, cwd, purpose)?;
	let value = stdout.trim();

	if value.is_empty() {
		return Err(format!("Failed to {purpose} with `{command}`: command returned no output."));
	}

	Ok(value.to_owned())
}

pub(in crate::agent::tracker_tool_bridge) fn run_command_for_stdout_allow_empty(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	run_command_stdout(command, args, cwd, purpose)
}

fn run_command_stdout(
	command: &str,
	args: &[&str],
	cwd: &Path,
	purpose: &str,
) -> std::result::Result<String, String> {
	let output = Command::new(command)
		.args(args)
		.current_dir(cwd)
		.output()
		.map_err(|error| format!("Failed to {purpose} with `{command}`: {error}"))?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);
		let stdout = String::from_utf8_lossy(&output.stdout);
		let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

		if detail.is_empty() {
			return Err(format!("Failed to {purpose} with `{command}`."));
		}

		return Err(format!("Failed to {purpose} with `{command}`: {detail}"));
	}

	let stdout = String::from_utf8_lossy(&output.stdout);

	Ok(stdout.into_owned())
}
