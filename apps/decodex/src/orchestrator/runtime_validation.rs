fn validate_review_handoff_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available("gh", "PR-backed review handoff")?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

fn validate_review_repair_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available("gh", "retained review-repair re-entry")?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

fn validate_closeout_runtime(
	project: &ServiceConfig,
	dry_run: bool,
) -> Result<()> {
	if dry_run {
		return Ok(());
	}

	validate_command_available("gh", "retained closeout re-entry")?;
	resolve_configured_env_var("github.token_env_var", Some(project.github().token_env_var()))?;

	Ok(())
}

fn validate_daemon_runtime() -> Result<()> {
	Ok(())
}

fn validate_command_available(command: &str, purpose: &str) -> Result<()> {
	let output = Command::new(command).arg("--version").output().map_err(|error| {
		eyre::eyre!("Required command `{command}` is unavailable for {purpose}: {error}")
	})?;

	if output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };

	if detail.is_empty() {
		eyre::bail!(
			"Required command `{command}` is unavailable for {purpose}: `{command} --version` exited unsuccessfully."
		);
	}

	eyre::bail!(
		"Required command `{command}` is unavailable for {purpose}: `{command} --version` failed with `{detail}`."
	);
}
