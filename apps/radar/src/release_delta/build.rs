//! Release-delta artifact construction and refresh entrypoints.

#[allow(clippy::wildcard_imports)] use super::*;

/// Refresh the stable-versus-prerelease release-delta artifact.
pub(crate) fn refresh_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
) -> crate::prelude::Result<RadarRefreshReleaseDeltaReport> {
	let root = repo_root()?;
	let api = GitHubApi::new(github_token(request.token_env.as_deref()))?;
	let payload = build_release_delta(request, &root, &api)?;
	let errors = validate_artifact_errors(&payload);

	if !errors.is_empty() {
		eyre::bail!("Release-delta validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", pretty_json(&payload)?);

		return Ok(release_delta_report(&payload, false, &root, &request.out));
	}

	let out = absolute_repo_path(&root, &request.out);
	let changed = write_json_if_material_changed(&out, &payload, RefreshKind::ReleaseDelta)?;

	Ok(release_delta_report(&payload, changed, &root, &request.out))
}

pub(crate) fn build_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	api: &GitHubApi,
) -> crate::prelude::Result<Value> {
	let releases = github_releases(api, &request.repo)?;
	let stable_release = select_release(&releases, &request.tag_prefix, false)?;
	let prerelease = select_release(&releases, &request.tag_prefix, true)?;
	let (stable_releases, preview_releases) = select_release_options(request, &releases)?;
	let release_pairs = select_release_pairs(
		request,
		root,
		&stable_release,
		&prerelease,
		&stable_releases,
		&preview_releases,
	)?;
	let signal_entries =
		load_signal_entries(&absolute_repo_path(root, &request.signals_dir), &request.repo)?;
	let mut comparison_entries = Vec::new();
	let mut default_tracked_signal_slugs = Vec::<String>::new();
	let mut default_compare_payload = None::<Value>;

	for pair in release_pairs {
		let is_default_pair = release_tag(&pair.stable) == release_tag(&stable_release)
			&& release_tag(&pair.preview) == release_tag(&prerelease);
		let comparison = build_release_comparison(api, request, &pair, &signal_entries)?;

		if is_default_pair {
			default_compare_payload = comparison.get("compare").cloned();
			default_tracked_signal_slugs = string_array_from_value(
				comparison.get("tracked_signal_slugs").unwrap_or(&Value::Null),
			);
		}

		comparison_entries.push(comparison);

		if request.pair_limit > 0
			&& comparison_entries.len() >= request.pair_limit
			&& default_compare_payload.is_some()
		{
			break;
		}
	}

	let Some(default_compare_payload) = default_compare_payload else {
		eyre::bail!("Default stable/prerelease pair was not included in comparison entries");
	};
	let (stable_options, preview_options) =
		filter_release_options(&stable_releases, &preview_releases, &comparison_entries);

	Ok(serde_json::json!({
		"schema": RELEASE_DELTA_SCHEMA,
		"repo": request.repo,
		"tag_prefix": request.tag_prefix,
		"generated_at": utc_now_iso()?,
		"stable_release": compact_release(&stable_release)?,
		"prerelease": compact_release(&prerelease)?,
		"compare": default_compare_payload,
		"release_options": {
			"stable": compact_releases(&stable_options)?,
			"preview": compact_releases(&preview_options)?,
		},
		"comparisons": comparison_entries,
		"tracked_signal_slugs": default_tracked_signal_slugs,
	}))
}

fn github_releases(api: &GitHubApi, repo: &str) -> crate::prelude::Result<Vec<Value>> {
	let mut releases = Vec::new();

	for page in 1..=5 {
		let payload = api
			.get(&format!("https://api.github.com/repos/{repo}/releases?per_page=100&page={page}"))?
			.payload;
		let Some(items) = payload.as_array() else {
			eyre::bail!("Expected releases list payload from GitHub API");
		};
		let count = items.len();

		releases.extend(items.iter().cloned());

		if count < 100 {
			break;
		}
	}

	Ok(releases)
}
