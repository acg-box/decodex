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
	let build_request = RadarBundleBuildRequest {
		repo: request.repo.clone(),
		pr: Some(pr_number),
		commit: None,
		force_commit_only: false,
		token_env: request.token_env.clone(),
		out: out.to_path_buf(),
		notes: vec![note.to_owned()],
	};
	let bundle = crate::operations::build_bundle_payload(&build_request)?;

	crate::write_json(out, &bundle)?;

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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn analysis_export_is_cleaned_and_output_changes_only_after_valid_success() {
		let temp = tempfile::tempdir().unwrap();
		let root = temp.path().join("repo with spaces");
		let helper = root.join(RUN_CODEX_ANALYSIS_SCRIPT);

		std::fs::create_dir_all(helper.parent().unwrap()).unwrap();
		std::fs::write(
			&helper,
			r#"
import argparse, json, pathlib, sys
parser = argparse.ArgumentParser()
parser.add_argument('--allow-ai-analysis-boundary', action='store_true')
parser.add_argument('--bundle')
parser.add_argument('--repo-root')
parser.add_argument('--codex-bin')
parser.add_argument('--model')
args = parser.parse_args()
root = pathlib.Path(args.repo_root).resolve()
bundle = pathlib.Path(args.bundle).resolve()
assert args.allow_ai_analysis_boundary
assert args.codex_bin == 'never-invoke-codex'
assert args.model == 'fixture-model'
assert pathlib.Path.cwd().resolve() == root
assert bundle.relative_to(root).parts[:2] == ('target', 'radar-analysis')
assert bundle.parent.stat().st_mode & 0o077 == 0
assert json.loads(bundle.read_text()) == json.loads((root / 'bundle.json').read_text())
mode = (root / 'mode').read_text()
if mode == 'fail':
    print('fixture helper failure', file=sys.stderr)
    sys.exit(17)
if mode == 'invalid-json':
    print('not JSON')
elif mode == 'invalid-draft':
    print('{}')
else:
    print((root / 'draft.json').read_text())
"#,
		)
		.unwrap();

		let bundle_path = root.join("bundle.json");
		let out = root.join("analysis.json");

		crate::write_json(&bundle_path, &crate::tests::fixtures::valid_bundle()).unwrap();

		let draft = crate::tests::fixtures::valid_signal();

		crate::write_json(&root.join("draft.json"), &draft).unwrap();

		let mut request = RadarBackfillReleaseRangeRequest {
			repo: "openai/codex".into(),
			release_delta: root.join("release.json"),
			stable_tag: None,
			preview_tag: None,
			signals_dir: root.join("signals"),
			bundles_dir: root.join("bundles"),
			analysis_dir: root.join("analysis"),
			token_env: None,
			codex_bin: "never-invoke-codex".into(),
			model: Some("fixture-model".into()),
			max_prs: None,
			dry_run: false,
			refresh_release_delta_first: false,
			refresh_stable_limit: None,
			refresh_preview_limit: None,
			refresh_pair_limit: None,
			python_bin: "python3".into(),
		};

		for (mode, expected_error) in [
			("success", None),
			("fail", Some("fixture helper failure")),
			("invalid-json", Some("returned invalid JSON")),
			("invalid-draft", Some("Analysis draft validation failed")),
		] {
			std::fs::write(root.join("mode"), mode).unwrap();

			let previous = b"previous output must survive";

			std::fs::write(&out, previous).unwrap();

			let result = run_codex_analysis(&root, &request, &bundle_path, &out);

			if let Some(message) = expected_error {
				assert!(result.unwrap_err().to_string().contains(message));
				assert_eq!(std::fs::read(&out).unwrap(), previous);
			} else {
				result.unwrap();

				assert_eq!(crate::load_json(&out).unwrap(), draft);
			}

			assert_eq!(std::fs::read_dir(root.join("target/radar-analysis")).unwrap().count(), 0);
		}

		request.python_bin = root.join("missing-helper").display().to_string();

		assert!(run_codex_analysis(&root, &request, &bundle_path, &out).is_err());
		assert_eq!(std::fs::read_dir(root.join("target/radar-analysis")).unwrap().count(), 0);
	}
}
