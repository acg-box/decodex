use crate::{
	prelude::Result,
	release_delta::{
		self, BTreeMap, BTreeSet, Path, RadarRefreshReleaseDeltaRequest, ReleasePair, Value, iter,
	},
};

pub(crate) fn select_release_pairs(
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
	let exists = if crate::is_radar_cache_path(path) {
		crate::private_file_exists(path)?
	} else {
		path.exists()
	};
	if !exists {
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
