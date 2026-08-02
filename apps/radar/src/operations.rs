//! Top-level Radar command operations.

use crate::{
	BUNDLE_SCHEMA, GitHubApi, GithubClient, Path, RadarBundleBuildReceipt, RadarBundleBuildRequest,
	RadarBundleValidateRequest, RadarRefreshQueueReport, RadarRefreshQueueRequest,
	RadarRenderSignalReport, RadarRenderSignalRequest, RadarValidateRequest, RadarValidationReport,
	RefreshKind, SIGNAL_SCHEMA, ValidationState, Value, eyre, prelude::Result,
};

pub(crate) fn refresh_queue(request: &RadarRefreshQueueRequest) -> Result<RadarRefreshQueueReport> {
	let root = crate::repo_root()?;
	let api = GitHubApi::new(crate::github_token(request.token_env.as_deref())?)?;
	let build = crate::build_review_queue(request, &root, &api)?;
	let errors = crate::validate_artifact_errors(&build.queue);

	if !errors.is_empty() {
		eyre::bail!("Upstream review queue validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", crate::pretty_json(&build.queue)?);
		let out = crate::absolute_repo_path(&root, &request.queue_out);
		let refresh = crate::inspect_json_refresh(&out, &build.queue, RefreshKind::Queue)?;

		return crate::queue_report(&build.queue, refresh, build.ledger_enabled);
	}

	let out = crate::absolute_repo_path(&root, &request.queue_out);
	let refresh = crate::refresh_json(&out, &build.queue, RefreshKind::Queue)?;

	crate::queue_report(&build.queue, refresh, build.ledger_enabled)
}

/// Validate the requested Radar artifact paths.
pub(crate) fn validate(request: &RadarValidateRequest) -> Result<RadarValidationReport> {
	let uses_default_paths = request.paths.is_empty();
	if request.bootstrap && !uses_default_paths {
		eyre::bail!(
			"RADAR_BOOTSTRAP_SCOPE: --bootstrap is valid only for the empty fixed generated cache"
		);
	}
	let paths = crate::validation_paths(&request.paths);
	if uses_default_paths {
		validate_default_cache_presence(Path::new("."), request.bootstrap)?;
	}
	let cache_gc = if uses_default_paths && !request.bootstrap {
		Some(crate::cache_gc(&crate::RadarCacheGcRequest::default())?)
	} else {
		None
	};
	let files = crate::collect_json_files(&paths, uses_default_paths)?;
	let max_age_hours = request
		.max_age_hours
		.or_else(|| uses_default_paths.then_some(crate::DEFAULT_SOURCE_MAX_AGE_HOURS));

	if max_age_hours == Some(0) {
		eyre::bail!("source freshness limit must be at least one hour");
	}
	let mut state = ValidationState::new();
	let mut errors = Vec::new();
	let now = crate::OffsetDateTime::now_utc();

	for path in &files {
		let payload = crate::load_json(path)?;
		let validation = crate::validate_artifact_for_path(path, &payload);

		if let Some(max_age_hours) = max_age_hours
			&& (!uses_default_paths || crate::is_default_source_snapshot(path))
		{
			crate::validate_source_freshness(path, &payload, max_age_hours, now, &mut errors);
		}
		if validation.schema.as_deref() == Some(SIGNAL_SCHEMA) {
			crate::validate_signal_slug_uniqueness(path, &payload, &mut state, &mut errors);
		}

		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
	}

	if errors.is_empty() {
		Ok(RadarValidationReport { checked_files: files.len(), cache_gc })
	} else {
		Err(eyre::eyre!("Radar validation failed:\n- {}", errors.join("\n- ")))
	}
}

pub(crate) fn validate_default_cache_presence(root: &Path, bootstrap: bool) -> Result<()> {
	if bootstrap {
		let cache_root = root.join(crate::DEFAULT_CACHE_ROOT);
		let cache = crate::private_fs::PrivateCache::open_or_create(&cache_root)?;
		let lock = cache.lock()?;

		if lock.bootstrap_cache_is_empty()? {
			return Ok(());
		}

		eyre::bail!(
			"RADAR_BOOTSTRAP_NONEMPTY: --bootstrap requires a completely empty generated cache"
		);
	}
	let cache_root = root.join(crate::DEFAULT_CACHE_ROOT);
	let cache = match crate::private_fs::PrivateCache::open_existing(&cache_root) {
		Ok(cache) => cache,
		Err(error)
			if error
				.chain()
				.find_map(|cause| cause.downcast_ref::<std::io::Error>())
				.is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
		{
			eyre::bail!(
				"Radar daily validation requires current source snapshots; generated cache is \
				 missing"
			);
		},
		Err(error) => return Err(error),
	};
	let lock = cache.lock()?;
	let missing = [
		("review_queue", crate::DEFAULT_QUEUE_OUT),
		("release_delta", crate::DEFAULT_RELEASE_DELTA_OUT),
	]
	.into_iter()
	.filter_map(|(label, path)| {
		Path::new(path)
			.strip_prefix(crate::DEFAULT_CACHE_ROOT)
			.ok()
			.map(|relative| (label, relative))
	})
	.map(|(label, relative)| {
		lock.cache().metadata(relative).map(|identity| identity.is_none().then_some(label))
	})
	.collect::<Result<Vec<_>>>()?
	.into_iter()
	.flatten()
	.collect::<Vec<_>>();

	if missing.is_empty() {
		Ok(())
	} else {
		eyre::bail!(
			"Radar daily validation requires current source snapshots; missing: {}. Run the \
			 refresh commands first, or use `radar validate --bootstrap` only for an explicit \
			 empty-cache bootstrap",
			missing.join(", ")
		);
	}
}

/// Build, install, and read back a deterministic GitHub change bundle.
pub(crate) fn build_bundle(request: &RadarBundleBuildRequest) -> Result<RadarBundleBuildReceipt> {
	validate_current_bundle_output_path(&request.out)?;
	let bundle = build_bundle_payload(request)?;

	crate::install_bundle(&request.out, &bundle)
}

pub(crate) fn build_bundle_payload(request: &RadarBundleBuildRequest) -> Result<Value> {
	let token = crate::github_token(request.token_env.as_deref())?;
	let client = GithubClient::new(token.as_deref())?;
	let bundle = match (request.pr, request.commit.as_deref()) {
		(Some(pr_number), _) => client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
		(None, Some(commit_sha)) => {
			let promoted_pr = if request.force_commit_only {
				None
			} else {
				client.maybe_promote_commit_to_pr(&request.repo, commit_sha)?
			};

			match promoted_pr {
				Some(pr_number) =>
					client.build_pr_bundle(&request.repo, pr_number, &request.notes)?,
				None => client.build_commit_bundle(&request.repo, commit_sha, &request.notes)?,
			}
		},
		(None, None) => eyre::bail!("one of --pr or --commit is required"),
	};

	Ok(bundle)
}

pub(crate) fn validate_current_bundle_output_path(path: &Path) -> Result<()> {
	let run_id = crate::current_run_id()?;
	let relative = crate::private_fs::private_cache_relative_path(path)?;
	let expected = Path::new("github/bundles").join(format!("{run_id}.json"));

	if relative != expected {
		eyre::bail!("bundle output path must match the current CODEX_THREAD_ID");
	}

	Ok(())
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
		Ok(RadarValidationReport { checked_files: files.len(), cache_gc: None })
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
