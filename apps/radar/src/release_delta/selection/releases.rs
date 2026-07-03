use crate::{
	prelude::Result,
	release_delta::{self, RadarRefreshReleaseDeltaRequest, Value, eyre},
};

pub(crate) fn select_release(
	releases: &[Value],
	tag_prefix: &str,
	prerelease: bool,
) -> Result<Value> {
	releases
		.iter()
		.find(|release| {
			!release.get("draft").and_then(Value::as_bool).unwrap_or(false)
				&& release_delta::release_tag(release)
					.is_some_and(|tag| tag.starts_with(tag_prefix))
				&& release.get("prerelease").and_then(Value::as_bool).unwrap_or(false) == prerelease
		})
		.cloned()
		.ok_or_else(|| {
			let kind = if prerelease { "prerelease" } else { "stable release" };

			eyre::eyre!("No {kind} found for tag prefix {tag_prefix:?}")
		})
}

pub(crate) fn select_release_options(
	request: &RadarRefreshReleaseDeltaRequest,
	releases: &[Value],
) -> Result<(Vec<Value>, Vec<Value>)> {
	let min_stable_key =
		release_delta::stable_version_key(&request.min_stable_tag, &request.tag_prefix);
	let mut stable = relevant_releases(releases, &request.tag_prefix)
		.into_iter()
		.filter(|release| {
			!release.get("prerelease").and_then(Value::as_bool).unwrap_or(false)
				&& release_delta::release_tag(release).is_some_and(|tag| {
					release_delta::stable_version_key(tag, &request.tag_prefix) >= min_stable_key
				})
		})
		.collect::<Vec<_>>();
	let mut preview = relevant_releases(releases, &request.tag_prefix)
		.into_iter()
		.filter(|release| release.get("prerelease").and_then(Value::as_bool).unwrap_or(false))
		.collect::<Vec<_>>();

	if request.stable_limit > 0 {
		stable.truncate(request.stable_limit);
	}
	if request.preview_limit > 0 {
		preview.truncate(request.preview_limit);
	}
	if stable.is_empty() {
		eyre::bail!(
			"No stable releases found for tag prefix {:?} at or above {:?}",
			request.tag_prefix,
			request.min_stable_tag
		);
	}
	if preview.is_empty() {
		eyre::bail!("No prereleases found for tag prefix {:?}", request.tag_prefix);
	}

	Ok((stable, preview))
}

fn relevant_releases(releases: &[Value], tag_prefix: &str) -> Vec<Value> {
	releases
		.iter()
		.filter(|release| {
			!release.get("draft").and_then(Value::as_bool).unwrap_or(false)
				&& release_delta::release_tag(release)
					.is_some_and(|tag| tag.starts_with(tag_prefix))
		})
		.cloned()
		.collect()
}
