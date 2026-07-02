use crate::orchestrator::*;

pub(in crate::orchestrator) fn validate_review_handoff_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available("gh", project.github().command_path(), "PR-backed review handoff")?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

pub(in crate::orchestrator) fn validate_review_repair_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available(
		"gh",
		project.github().command_path(),
		"retained review-repair re-entry",
	)?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

pub(in crate::orchestrator) fn validate_closeout_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available(
		"gh",
		project.github().command_path(),
		"retained closeout re-entry",
	)?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

pub(in crate::orchestrator) fn validate_daemon_runtime() -> Result<()> {
	Ok(())
}

pub(in crate::orchestrator) fn validate_command_available(
	command: &str,
	configured_path: Option<&Path>,
	purpose: &str,
) -> Result<()> {
	let mut command_runner = if command == "gh" {
		github::gh_command_with_config(configured_path)
	} else {
		Command::new(command)
	};
	let command_label = command_runner.get_program().to_string_lossy().into_owned();
	let output = command_runner.arg("--version").output().map_err(|error| {
		eyre::eyre!("Required command `{command_label}` is unavailable for {purpose}: {error}")
	})?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!(
			"Required command `{command_label}` is unavailable for {purpose}: `{command_label} --version` exited unsuccessfully."
		);
	}

	eyre::bail!(
		"Required command `{command_label}` is unavailable for {purpose}: `{command_label} --version` failed with `{detail}`."
	);
}
