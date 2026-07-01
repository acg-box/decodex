use std::{
	collections::BTreeSet,
	env, fs,
	path::{Path, PathBuf},
	process::{self, Command},
};

use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::prelude::eyre;

use super::{
	super::{
		RELEASE_DELTA_SCHEMA, RUN_CODEX_ANALYSIS_SCRIPT, RadarBackfillReleaseRangeReport,
		RadarBackfillReleaseRangeRequest, RadarBundleBuildRequest, RadarRefreshReleaseDeltaRequest,
		RadarRenderSignalRequest, RadarValidateRequest, SIGNAL_SCHEMA, build_bundle, load_json,
		path_arg, render_signal, repo_root, resolve_against, string_field, validate,
		validate_expected_schema,
	},
	refresh_release_delta,
};

#[derive(Debug)]
struct PreparedReleaseDelta {
	path: PathBuf,
	cleanup_dir: Option<PathBuf>,
}
impl Drop for PreparedReleaseDelta {
	fn drop(&mut self) {
		if let Some(path) = &self.cleanup_dir {
			let _ = fs::remove_dir_all(path);
		}
	}
}

#[derive(Debug, Eq, PartialEq)]
struct ReleaseSelection {
	stable_tag: String,
	preview_tag: String,
	pr_numbers: Vec<u64>,
}

#[derive(Debug)]
struct BackfillPaths {
	bundle: PathBuf,
	analysis: PathBuf,
	signal: PathBuf,
}

/// Select and optionally execute release-window signal backfills.
pub(crate) fn backfill_release_range(
	request: &RadarBackfillReleaseRangeRequest,
) -> crate::prelude::Result<RadarBackfillReleaseRangeReport> {
	let root = repo_root()?;
	let prepared_release_delta = prepare_release_delta_path(request, &root)?;
	let release_delta = load_json(&prepared_release_delta.path)?;
	let selection = selected_release_comparison(
		&release_delta,
		request.stable_tag.as_deref(),
		request.preview_tag.as_deref(),
	)?;
	let signals_dir = resolve_against(&root, &request.signals_dir);
	let published = published_pr_numbers(&signals_dir)?;
	let mut target_prs = selection
		.pr_numbers
		.into_iter()
		.filter(|number| !published.contains(number))
		.collect::<Vec<_>>();

	if let Some(limit) = request.max_prs {
		target_prs.truncate(limit);
	}

	let mut report = RadarBackfillReleaseRangeReport {
		stable_tag: selection.stable_tag,
		preview_tag: selection.preview_tag,
		target_prs,
		created: 0,
		dry_run: request.dry_run,
	};

	if request.dry_run {
		return Ok(report);
	}

	for pr_number in &report.target_prs {
		let paths = signal_backfill_paths(&request.repo, *pr_number, request);
		let note = format!(
			"Backfilled from release compare range {}...{}",
			report.stable_tag, report.preview_tag
		);
		let bundle_path = resolve_against(&root, &paths.bundle);
		let analysis_path = resolve_against(&root, &paths.analysis);
		let signal_path = resolve_against(&root, &paths.signal);

		run_build_bundle(request, *pr_number, &bundle_path, &note)?;
		run_codex_analysis(&root, request, &bundle_path, &analysis_path)?;
		render_signal(&RadarRenderSignalRequest {
			bundle: bundle_path,
			analysis: analysis_path,
			out: signal_path,
			published_at: None,
		})?;

		report.created += 1;
	}

	validate(&RadarValidateRequest { paths: vec![resolve_against(&root, &request.signals_dir)] })?;
	run_refresh_release_delta(request, &request.release_delta, false)?;

	Ok(report)
}

fn selected_release_comparison(
	payload: &Value,
	stable_tag: Option<&str>,
	preview_tag: Option<&str>,
) -> crate::prelude::Result<ReleaseSelection> {
	validate_expected_schema(payload, RELEASE_DELTA_SCHEMA, "Release-delta")?;

	let entry =
		payload.as_object().ok_or_else(|| eyre::eyre!("Release-delta must be an object"))?;
	let target_stable = stable_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("stable_release")))
		.ok_or_else(|| eyre::eyre!("stable release tag could not be selected"))?;
	let target_preview = preview_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("prerelease")))
		.ok_or_else(|| eyre::eyre!("preview release tag could not be selected"))?;
	let comparisons = entry
		.get("comparisons")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Release-delta comparisons must be a list"))?;

	for comparison in comparisons {
		let Some(comparison) = comparison.as_object() else {
			continue;
		};

		if string_field(comparison, "stable_tag_name") == Some(target_stable.as_str())
			&& string_field(comparison, "prerelease_tag_name") == Some(target_preview.as_str())
		{
			return Ok(ReleaseSelection {
				stable_tag: target_stable,
				preview_tag: target_preview,
				pr_numbers: comparison_pr_numbers(comparison),
			});
		}
	}

	Err(eyre::eyre!("No comparison found for {target_stable} -> {target_preview}"))
}

fn release_delta_release_tag(value: Option<&Value>) -> Option<String> {
	value
		.and_then(Value::as_object)
		.and_then(|release| string_field(release, "tag_name"))
		.filter(|tag| !tag.is_empty())
		.map(str::to_owned)
}

fn comparison_pr_numbers(comparison: &Map<String, Value>) -> Vec<u64> {
	comparison
		.get("compare")
		.and_then(Value::as_object)
		.and_then(|compare| compare.get("pr_numbers"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_u64)
		.collect()
}

fn published_pr_numbers(signals_dir: &Path) -> crate::prelude::Result<BTreeSet<u64>> {
	let mut published = BTreeSet::new();
	let mut files = Vec::new();

	for entry in fs::read_dir(signals_dir)? {
		let path = entry?.path();

		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path);
		}
	}

	files.sort();

	for path in files {
		let payload = load_json(&path)?;

		validate_expected_schema(&payload, SIGNAL_SCHEMA, "Signal")?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(Value::as_object)
			.and_then(|refs| string_field(refs, "pr_url"))
			.and_then(pr_number_from_url)
		{
			published.insert(pr_number);
		}
	}

	Ok(published)
}

fn pr_number_from_url(value: &str) -> Option<u64> {
	let marker = "/pull/";
	let index = value.rfind(marker)?;
	let number = &value[index + marker.len()..];

	(!number.is_empty() && number.chars().all(|character| character.is_ascii_digit()))
		.then(|| number.parse().ok())
		.flatten()
}

fn prepare_release_delta_path(
	request: &RadarBackfillReleaseRangeRequest,
	root: &Path,
) -> crate::prelude::Result<PreparedReleaseDelta> {
	if !request.refresh_release_delta_first {
		return Ok(PreparedReleaseDelta {
			path: resolve_against(root, &request.release_delta),
			cleanup_dir: None,
		});
	}

	let temp_root = env::temp_dir().join(format!(
		"decodex-prerelease-delta-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));

	fs::create_dir_all(&temp_root)?;

	let release_delta = temp_root.join("release-delta.json");

	run_refresh_release_delta(request, &release_delta, true)?;

	Ok(PreparedReleaseDelta { path: release_delta, cleanup_dir: Some(temp_root) })
}

fn run_build_bundle(
	request: &RadarBackfillReleaseRangeRequest,
	pr_number: u64,
	out: &Path,
	note: &str,
) -> crate::prelude::Result<()> {
	build_bundle(&RadarBundleBuildRequest {
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

fn run_codex_analysis(
	root: &Path,
	request: &RadarBackfillReleaseRangeRequest,
	bundle: &Path,
	out: &Path,
) -> crate::prelude::Result<()> {
	let mut command = helper_command(root, request, RUN_CODEX_ANALYSIS_SCRIPT);

	command.arg("--allow-ai-analysis-boundary");
	command.args([
		"--bundle",
		&path_arg(root, bundle),
		"--out",
		&path_arg(root, out),
		"--repo-root",
		&root.display().to_string(),
		"--codex-bin",
		request.codex_bin.as_str(),
	]);

	if let Some(model) = &request.model {
		command.args(["--model", model]);
	}

	run_helper(command, RUN_CODEX_ANALYSIS_SCRIPT)
}

fn run_refresh_release_delta(
	request: &RadarBackfillReleaseRangeRequest,
	out: &Path,
	include_refresh_limits: bool,
) -> crate::prelude::Result<()> {
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

	refresh_release_delta(&refresh_request)?;

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

fn run_helper(mut command: Command, script: &str) -> crate::prelude::Result<()> {
	let output = command.output()?;

	if output.status.success() {
		return Ok(());
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

fn signal_backfill_paths(
	repo: &str,
	pr_number: u64,
	request: &RadarBackfillReleaseRangeRequest,
) -> BackfillPaths {
	let stem = format!("{}-pr-{pr_number}", repo_path_stem(repo));

	BackfillPaths {
		bundle: request.bundles_dir.join(format!("{stem}.json")),
		analysis: request.analysis_dir.join(format!("{stem}.analysis.json")),
		signal: request.signals_dir.join(format!("{stem}.json")),
	}
}

fn repo_path_stem(repo: &str) -> String {
	repo.chars()
		.map(
			|character| {
				if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
			},
		)
		.collect::<String>()
		.trim_matches('-')
		.to_owned()
}
