//! Top-level Radar command operations.

use crate::{
	BUNDLE_SCHEMA, GitHubApi, GithubClient, PathBuf, RadarBundleBuildRequest,
	RadarBundleValidateRequest, RadarRefreshQueueReport, RadarRefreshQueueRequest,
	RadarRenderSignalReport, RadarRenderSignalRequest, RadarValidateRequest, RadarValidationReport,
	RefreshKind, SIGNAL_SCHEMA, ValidationState, eyre, prelude::Result,
};

pub(crate) fn refresh_queue(request: &RadarRefreshQueueRequest) -> Result<RadarRefreshQueueReport> {
	let root = crate::repo_root()?;
	let api = GitHubApi::new(crate::github_token(request.token_env.as_deref()))?;
	let build = crate::build_review_queue(request, &root, &api)?;
	let errors = crate::validate_artifact_errors(&build.queue);

	if !errors.is_empty() {
		eyre::bail!("Upstream review queue validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", crate::pretty_json(&build.queue)?);

		return Ok(crate::queue_report(
			&build.queue,
			false,
			build.ledger_enabled,
			&root,
			&request.queue_out,
		));
	}

	let out = crate::absolute_repo_path(&root, &request.queue_out);
	let changed = crate::write_json_if_material_changed(&out, &build.queue, RefreshKind::Queue)?;

	Ok(crate::queue_report(&build.queue, changed, build.ledger_enabled, &root, &request.queue_out))
}

/// Validate the requested Radar artifact paths.
pub(crate) fn validate(request: &RadarValidateRequest) -> Result<RadarValidationReport> {
	let paths = crate::validation_paths(&request.paths);
	let files = crate::collect_json_files(&paths)?;
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = crate::load_json(path)?;
		let validation = crate::validate_artifact_for_path(path, &payload);

		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			crate::validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Radar validation failed:\n- {}", errors.join("\n- ")))
	}
}

/// Build a deterministic GitHub change bundle and write it to disk.
pub(crate) fn build_bundle(request: &RadarBundleBuildRequest) -> Result<PathBuf> {
	let token = crate::github_token(request.token_env.as_deref());
	let client = GithubClient::new(token.as_deref())?;
	let bundle = match (request.pr, request.commit.as_deref()) {
		(Some(pr_number), _) => client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
		(None, Some(commit_sha)) => {
			let promoted_pr = if request.force_commit_only {
				None
			} else {
				client.maybe_promote_commit_to_pr(&request.repo, commit_sha)
			};

			match promoted_pr {
				Some(pr_number) => {
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?
				},
				None => client.build_commit_bundle(&request.repo, commit_sha, &request.notes)?,
			}
		},
		(None, None) => eyre::bail!("one of --pr or --commit is required"),
	};

	crate::write_json(&request.out, &bundle)?;

	Ok(request.out.clone())
}

/// Validate GitHub change bundle artifacts only.
pub(crate) fn validate_bundles(
	request: &RadarBundleValidateRequest,
) -> Result<RadarValidationReport> {
	let files = crate::collect_bundle_json_files(&request.paths)?;
	let mut errors = Vec::new();

	for path in &files {
		let payload = crate::load_json(path)?;
		let validation = crate::validate_artifact(&payload);

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.push(format!("{}: schema must be {BUNDLE_SCHEMA}", path.display()));
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len() })
	} else {
		Err(eyre::eyre!("Bundle validation failed:\n- {}", errors.join("\n- ")))
	}
}

/// Render one `signal_entry/v1` artifact from a validated bundle and analysis draft.
pub(crate) fn render_signal(request: &RadarRenderSignalRequest) -> Result<RadarRenderSignalReport> {
	let bundle = crate::load_json(&request.bundle)?;
	let analysis = crate::load_json(&request.analysis)?;

	crate::validate_expected_schema(&bundle, BUNDLE_SCHEMA, "Bundle")?;
	crate::validate_analysis_draft(&analysis)?;

	let root = crate::repo_root()?;
	let known_features = crate::load_known_feature_names(&root)?;
	let config_flags = crate::rendered_config_flags(&bundle, &analysis, &known_features);
	let signal =
		crate::rendered_signal(&bundle, &analysis, request.published_at.as_deref(), config_flags)?;

	crate::validate_expected_schema(&signal, SIGNAL_SCHEMA, "Signal")?;
	crate::write_json(&request.out, &signal)?;

	Ok(RadarRenderSignalReport { out: request.out.clone() })
}
