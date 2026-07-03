//! Release and comparison pair selection.

use crate::{
	prelude::Result,
	release_delta::{
		self, BTreeMap, BTreeSet, Path, RadarRefreshReleaseDeltaRequest, ReleasePair, Value, eyre,
		iter,
	},
};

pub(super) fn select_release(
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

pub(super) fn select_release_options(
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

pub(super) fn select_release_pairs(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	stable_release: &Value,
	prerelease: &Value,
	stable_releases: &[Value],
	preview_releases: &[Value],
) -> Result<Vec<ReleasePair>> {
	let default_pair = ReleasePair { stable: stable_release.clone(), preview: prerelease.clone() };
	let releases_by_tag = stable_releases
		.iter()
		.chain(preview_releases)
		.filter_map(|release| {
			release_delta::release_tag(release).map(|tag| (tag.to_owned(), release.clone()))
		})
		.collect::<BTreeMap<_, _>>();
	let previous_pairs =
		previous_signal_pairs(&release_delta::absolute_repo_path(root, &request.out))?
			.into_iter()
			.filter_map(|(stable_tag, preview_tag)| {
				Some(ReleasePair {
					stable: releases_by_tag.get(&stable_tag)?.clone(),
					preview: releases_by_tag.get(&preview_tag)?.clone(),
				})
			})
			.collect::<Vec<_>>();

	if previous_pairs.is_empty() {
		let mut pairs = vec![default_pair];

		pairs.extend(compare_candidates(stable_releases, preview_releases));

		let mut pairs = unique_release_pairs(pairs);

		if request.pair_limit > 0 {
			pairs.truncate(request.pair_limit);
		}

		Ok(pairs)
	} else {
		Ok(unique_release_pairs(iter::once(default_pair).chain(previous_pairs).collect()))
	}
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

fn compare_candidates(stable_releases: &[Value], preview_releases: &[Value]) -> Vec<ReleasePair> {
	let mut candidates = stable_releases
		.iter()
		.flat_map(|stable| {
			preview_releases
				.iter()
				.filter(move |preview| {
					release_delta::release_sort_key(preview)
						> release_delta::release_sort_key(stable)
				})
				.map(move |preview| ReleasePair {
					stable: stable.clone(),
					preview: preview.clone(),
				})
		})
		.collect::<Vec<_>>();

	candidates.sort_by(|left, right| {
		(
			release_delta::release_sort_key(&right.preview),
			release_delta::release_sort_key(&right.stable),
		)
			.cmp(&(
				release_delta::release_sort_key(&left.preview),
				release_delta::release_sort_key(&left.stable),
			))
	});

	candidates
}

fn unique_release_pairs(pairs: Vec<ReleasePair>) -> Vec<ReleasePair> {
	let mut seen = BTreeSet::new();
	let mut unique = Vec::new();

	for pair in pairs {
		let Some(stable_tag) = release_delta::release_tag(&pair.stable) else {
			continue;
		};
		let Some(preview_tag) = release_delta::release_tag(&pair.preview) else {
			continue;
		};
		let key = (stable_tag.to_owned(), preview_tag.to_owned());

		if seen.insert(key) {
			unique.push(pair);
		}
	}

	unique
}

fn previous_signal_pairs(path: &Path) -> Result<Vec<(String, String)>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	let Ok(previous) = release_delta::load_json(path) else {
		return Ok(Vec::new());
	};
	let mut keys = Vec::new();
	let mut seen = BTreeSet::new();

	for comparison in previous.get("comparisons").and_then(Value::as_array).into_iter().flatten() {
		if release_delta::string_array(comparison.get("tracked_signal_slugs")).is_empty() {
			continue;
		}

		let stable_tag = comparison.get("stable_tag_name").and_then(Value::as_str);
		let preview_tag = comparison.get("prerelease_tag_name").and_then(Value::as_str);
		let (Some(stable_tag), Some(preview_tag)) = (stable_tag, preview_tag) else {
			continue;
		};
		let key = (stable_tag.to_owned(), preview_tag.to_owned());

		if seen.insert(key.clone()) {
			keys.push(key);
		}
	}

	Ok(keys)
}
