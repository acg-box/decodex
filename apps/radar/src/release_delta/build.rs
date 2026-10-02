//! Release-delta artifact construction and refresh entrypoints.

use crate::{
	prelude::Result,
	release_delta::{
		self, GitHubApi, Path, RELEASE_DELTA_SCHEMA, RadarRefreshReleaseDeltaReport,
		RadarRefreshReleaseDeltaRequest, RefreshKind, Value, eyre, serde_json,
	},
};

/// Refresh the stable-versus-prerelease release-delta artifact.
pub(crate) fn refresh_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
) -> Result<RadarRefreshReleaseDeltaReport> {
	let root = release_delta::repo_root()?;
	let api = GitHubApi::new(release_delta::github_token(request.token_env.as_deref())?)?;
	let payload = build_release_delta(request, &root, &api)?;
	let errors = release_delta::validate_artifact_errors(&payload);

	if !errors.is_empty() {
		eyre::bail!("Release-delta validation failed:\n- {}", errors.join("\n- "));
	}
	if request.dry_run {
		println!("{}", release_delta::pretty_json(&payload)?);

		let out = release_delta::absolute_repo_path(&root, &request.out);
		let refresh =
			release_delta::inspect_json_refresh(&out, &payload, RefreshKind::ReleaseDelta)?;

		return Ok(release_delta::release_delta_report(&payload, refresh));
	}

	let out = release_delta::absolute_repo_path(&root, &request.out);
	let refresh = release_delta::refresh_json(&out, &payload, RefreshKind::ReleaseDelta)?;

	Ok(release_delta::release_delta_report(&payload, refresh))
}

pub(crate) fn build_release_delta(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	api: &GitHubApi,
) -> Result<Value> {
	let releases =
		github_releases(api, &format!("https://api.github.com/repos/{}/releases", request.repo))?;
	let stable_release = release_delta::select_release(&releases, &request.tag_prefix, false)?;
	let prerelease = release_delta::select_release(&releases, &request.tag_prefix, true)?;
	let (stable_releases, preview_releases) =
		release_delta::select_release_options(request, &releases)?;
	let release_pairs = release_delta::select_release_pairs(
		request,
		root,
		&stable_release,
		&prerelease,
		&stable_releases,
		&preview_releases,
	)?;
	let signal_entries = release_delta::load_signal_entries(
		&release_delta::absolute_repo_path(root, &request.signals_dir),
		&request.repo,
	)?;
	let mut comparison_entries = Vec::new();
	let mut default_tracked_signal_slugs = Vec::<String>::new();
	let mut default_compare_payload = None::<Value>;

	for pair in release_pairs {
		let is_default_pair = release_delta::release_tag(&pair.stable)
			== release_delta::release_tag(&stable_release)
			&& release_delta::release_tag(&pair.preview) == release_delta::release_tag(&prerelease);
		let comparison =
			release_delta::build_release_comparison(api, request, &pair, &signal_entries)?;

		if is_default_pair {
			default_compare_payload = comparison.get("compare").cloned();
			default_tracked_signal_slugs = release_delta::string_array_from_value(
				comparison.get("tracked_signal_slugs").unwrap_or(&Value::Null),
			);
		}

		comparison_entries.push(comparison);
	}

	let Some(default_compare_payload) = default_compare_payload else {
		eyre::bail!("Default stable/prerelease pair was not included in comparison entries");
	};
	let (stable_options, preview_options) = release_delta::filter_release_options(
		&stable_releases,
		&preview_releases,
		&comparison_entries,
	);

	Ok(serde_json::json!({
		"schema": RELEASE_DELTA_SCHEMA,
		"repo": request.repo,
		"tag_prefix": request.tag_prefix,
		"generated_at": release_delta::utc_now_iso()?,
		"stable_release": release_delta::compact_release(&stable_release)?,
		"prerelease": release_delta::compact_release(&prerelease)?,
		"compare": default_compare_payload,
		"release_options": {
			"stable": release_delta::compact_releases(&stable_options)?,
			"preview": release_delta::compact_releases(&preview_options)?,
		},
		"comparisons": comparison_entries,
		"tracked_signal_slugs": default_tracked_signal_slugs,
	}))
}

fn github_releases(api: &GitHubApi, url: &str) -> Result<Vec<Value>> {
	api.get_paginated(&format!("{url}?per_page=100"))
}

#[cfg(test)]
mod tests {
	use crate::{
		RadarRefreshReleaseDeltaRequest,
		release_delta::{self, build},
		tests::{automation::github_api, fixtures},
	};

	#[test]
	fn release_catalog_keeps_stable_releases_beyond_the_fifth_page() {
		let server = github_api::spawn_server_with(6, |url, page| {
			let start = page * 100;
			let releases = (start..(start + 100).min(501))
				.map(|index| {
					let tag = if index == 500 {
						"rust-v0.116.0".to_owned()
					} else {
						format!("rust-v0.117.0-alpha.{index}")
					};

					fixtures::release(&tag, index != 500)
				})
				.collect::<Vec<_>>();
			let body = serde_json::to_string(&releases).unwrap();

			if page < 5 {
				github_api::response(
					"200 OK",
					&[("Link", &format!("<{url}?per_page=100&page={}>; rel=\"next\"", page + 2))],
					&body,
				)
			} else {
				github_api::response("200 OK", &[], &body)
			}
		});
		let releases = build::github_releases(&server.api(None), server.url()).unwrap();

		assert_eq!(releases.len(), 501);

		let stable = release_delta::select_release(&releases, "rust-v", false).unwrap();

		assert_eq!(stable["tag_name"], "rust-v0.116.0");

		let mut request = RadarRefreshReleaseDeltaRequest {
			stable_limit: 0,
			preview_limit: 0,
			..Default::default()
		};
		let (stable, preview) = release_delta::select_release_options(&request, &releases).unwrap();

		assert_eq!((stable.len(), preview.len()), (1, 500));

		request.preview_limit = 2;

		let (stable, preview) = release_delta::select_release_options(&request, &releases).unwrap();

		assert_eq!((stable.len(), preview.len()), (1, 2));
		assert_eq!(server.finish_with_requests().len(), 6);
	}
}
