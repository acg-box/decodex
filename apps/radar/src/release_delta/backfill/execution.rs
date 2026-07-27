use std::{path::Path, process::Command};

use crate::{
	RUN_CODEX_ANALYSIS_SCRIPT, RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest,
	RadarRefreshReleaseDeltaRequest,
	prelude::{Result, eyre},
	release_delta,
};

pub(in crate::release_delta::backfill) fn run_build_bundle(
	request: &RadarBackfillReleaseRangeRequest,
	pr_number: u64,
	out: &Path,
	note: &str,
) -> Result<()> {
	crate::build_bundle(&RadarBundleBuildRequest {
		repo: request.repo.clone(),
		pr: Some(pr_number),
		commit: None,
		force_commit_only: false,
		token_env: request.token_env.clone(),
		out: out.to_path_buf(),
		notes: vec![note.to_owned()],
	})?;

	Ok(())
}

pub(in crate::release_delta::backfill) fn run_codex_analysis(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	bundle: &Path,
	out: &Path,
) -> Result<()> {
	let bundle_payload = crate::load_json(bundle)?;
	crate::validate_expected_schema(&bundle_payload, crate::BUNDLE_SCHEMA, "Bundle")?;
	let temp_parent = root.join("target/radar-analysis");

	std::fs::create_dir_all(&temp_parent)?;
	let temp_dir = tempfile::tempdir_in(temp_parent)?;
	let copied_bundle = temp_dir.path().join("bundle.json");

	crate::write_json(&copied_bundle, &bundle_payload)?;

	let mut command = helper_command(root, request, RUN_CODEX_ANALYSIS_SCRIPT);

	command.arg("--allow-ai-analysis-boundary");
	command.args([
		"--bundle",
		&crate::path_arg(root, &copied_bundle),
		"--repo-root",
		&root.display().to_string(),
		"--codex-bin",
		request.codex_bin.as_str(),
	]);

	if let Some(model) = &request.model {
		command.args(["--model", model]);
	}

	let output = run_helper(command, RUN_CODEX_ANALYSIS_SCRIPT)?;
	let payload: serde_json::Value = serde_json::from_slice(&output).map_err(|error| {
		eyre::eyre!("{RUN_CODEX_ANALYSIS_SCRIPT} returned invalid JSON: {error}")
	})?;

	crate::validate_analysis_draft(&payload)?;
	crate::write_json(out, &payload)
}

pub(in crate::release_delta::backfill) fn run_refresh_release_delta(
	request: &RadarBackfillReleaseRangeRequest,
	out: &Path,
	include_refresh_limits: bool,
) -> Result<()> {
	let mut refresh_request = RadarRefreshReleaseDeltaRequest {
		repo: request.repo.clone(),
		signals_dir: request.signals_dir.clone(),
		out: out.to_path_buf(),
		token_env: request.token_env.clone(),
		..RadarRefreshReleaseDeltaRequest::default()
	};

	if include_refresh_limits {
		if let Some(limit) = request.refresh_stable_limit {
			refresh_request.stable_limit = limit;
		}
		if let Some(limit) = request.refresh_preview_limit {
			refresh_request.preview_limit = limit;
		}
		if let Some(limit) = request.refresh_pair_limit {
			refresh_request.pair_limit = limit;
		}
	}

	release_delta::refresh_release_delta(&refresh_request)?;

	Ok(())
}

fn helper_command(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	script: &str,
) -> Command {
	let mut command = Command::new(&request.python_bin);

	command.current_dir(root).arg(root.join(script));

	command
}

fn run_helper(mut command: Command, script: &str) -> Result<Vec<u8>> {
	let output = command.output()?;

	if output.status.success() {
		return Ok(output.stdout);
	}

	let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
	let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
	let details = if !stderr.is_empty() {
		stderr
	} else if !stdout.is_empty() {
		stdout
	} else {
		"unknown error".into()
	};

	Err(eyre::eyre!("{script} failed: {details}"))
}
