//! Top-level Radar command operations.

#[allow(clippy::wildcard_imports)] use super::*;

pub(crate) fn refresh_queue(
	request: &RadarRefreshQueueRequest,
) -> crate::prelude::Result<RadarRefreshQueueReport> {
	let root = repo_root()?;
	let api = GitHubApi::new(github_token(request.token_env.as_deref()))?;
	let build = build_review_queue(request, &root, &api)?;
	let errors = validate_artifact_errors(&build.queue);

	if !errors.is_empty() {
		eyre::bail!("Upstream review queue validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", pretty_json(&build.queue)?);

		return Ok(queue_report(
			&build.queue,
			false,
			build.ledger_enabled,
			&root,
			&request.queue_out,
		));
	}

	let out = absolute_repo_path(&root, &request.queue_out);
	let changed = write_json_if_material_changed(&out, &build.queue, RefreshKind::Queue)?;

	Ok(queue_report(&build.queue, changed, build.ledger_enabled, &root, &request.queue_out))
}

/// Validate the requested Radar artifact paths.
pub(crate) fn validate(
	request: &RadarValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let paths = validation_paths(&request.paths);
	let files = collect_json_files(&paths)?;
	let mut state = ValidationState::new();
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact_for_path(path, &payload);

		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
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
pub(crate) fn build_bundle(request: &RadarBundleBuildRequest) -> crate::prelude::Result<PathBuf> {
	let token = github_token(request.token_env.as_deref());
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
				Some(pr_number) =>
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
				None => client.build_commit_bundle(&request.repo, commit_sha, &request.notes)?,
			}
		},
		(None, None) => eyre::bail!("one of --pr or --commit is required"),
	};

	write_json(&request.out, &bundle)?;

	Ok(request.out.clone())
}

/// Validate GitHub change bundle artifacts only.
pub(crate) fn validate_bundles(
	request: &RadarBundleValidateRequest,
) -> crate::prelude::Result<RadarValidationReport> {
	let files = collect_bundle_json_files(&request.paths)?;
	let mut errors = Vec::new();

	for path in &files {
		let payload = load_json(path)?;
		let validation = validate_artifact(&payload);

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
pub(crate) fn render_signal(
	request: &RadarRenderSignalRequest,
) -> crate::prelude::Result<RadarRenderSignalReport> {
	let bundle = load_json(&request.bundle)?;
	let analysis = load_json(&request.analysis)?;

	validate_expected_schema(&bundle, BUNDLE_SCHEMA, "Bundle")?;
	validate_analysis_draft(&analysis)?;

	let root = repo_root()?;
	let known_features = load_known_feature_names(&root)?;
	let config_flags = rendered_config_flags(&bundle, &analysis, &known_features);
	let signal =
		rendered_signal(&bundle, &analysis, request.published_at.as_deref(), config_flags)?;

	validate_expected_schema(&signal, SIGNAL_SCHEMA, "Signal")?;
	write_json(&request.out, &signal)?;

	Ok(RadarRenderSignalReport { out: request.out.clone() })
}
