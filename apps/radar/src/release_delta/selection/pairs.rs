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
	let previous_pairs = previous_signal_pairs(
		&release_delta::absolute_repo_path(root, &request.out),
		&request.repo,
	)?
	.into_iter()
	.filter_map(|(stable_tag, preview_tag)| {
		Some(ReleasePair {
			stable: releases_by_tag.get(&stable_tag)?.clone(),
			preview: releases_by_tag.get(&preview_tag)?.clone(),
		})
	})
	.collect::<Vec<_>>();
	let candidates = if previous_pairs.is_empty() {
		compare_candidates(stable_releases, preview_releases)
	} else {
		previous_pairs
	};
	let mut pairs = unique_release_pairs(iter::once(default_pair).chain(candidates).collect());

	if request.pair_limit > 0 {
		pairs.truncate(request.pair_limit);
	}

	Ok(pairs)
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

fn previous_signal_pairs(path: &Path, repo: &str) -> Result<Vec<(String, String)>> {
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

	if previous.get("repo").and_then(Value::as_str) != Some(repo) {
		return Ok(Vec::new());
	}

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

#[cfg(test)]
mod tests {
	use std::fs;

	use crate::{
		RadarRefreshReleaseDeltaRequest,
		release_delta::{self, selection::pairs},
		tests::fixtures,
	};

	#[test]
	fn release_pairs_reuse_only_history_for_the_requested_repository() {
		let temp = tempfile::tempdir().unwrap();
		let out = temp.path().join("release-delta.json");
		let release = |tag: &str, preview: bool, published_at: &str| {
			let mut payload = fixtures::release(tag, preview);

			payload["published_at"] = serde_json::json!(published_at);

			payload
		};
		let stable = [
			release("rust-v0.2.0", false, "2026-01-02T00:00:00Z"),
			release("rust-v0.1.0", false, "2026-01-01T00:00:00Z"),
		];
		let preview = [
			release("rust-v0.4.0-alpha.1", true, "2026-03-01T00:00:00Z"),
			release("rust-v0.3.0-alpha.1", true, "2026-02-01T00:00:00Z"),
		];
		let select = |pair_limit| {
			let request = RadarRefreshReleaseDeltaRequest {
				out: out.clone(),
				pair_limit,
				..Default::default()
			};

			pairs::select_release_pairs(
				&request,
				temp.path(),
				&stable[0],
				&preview[0],
				&stable,
				&preview,
			)
			.unwrap()
			.into_iter()
			.map(|pair| {
				(
					release_delta::required_release_tag(&pair.stable).unwrap().to_owned(),
					release_delta::required_release_tag(&pair.preview).unwrap().to_owned(),
				)
			})
			.collect::<Vec<_>>()
		};
		let fresh = select(0);

		assert_eq!(fresh.len(), 4);
		assert_eq!(select(1), fresh[..1]);
		assert_eq!(select(2), fresh[..2]);
		assert_eq!(select(10), fresh);

		for repo in [
			serde_json::json!("other/project"),
			serde_json::Value::Null,
			serde_json::json!("openai/codex"),
		] {
			let previous = serde_json::json!({
				"repo": repo,
				"comparisons": [{
					"stable_tag_name": "rust-v0.1.0",
					"prerelease_tag_name": "rust-v0.3.0-alpha.1",
					"tracked_signal_slugs": ["signal"]
				}]
			});

			crate::write_json(&out, &previous).unwrap();

			let pairs = select(0);

			if repo == "openai/codex" {
				assert_eq!(
					pairs,
					vec![fresh[0].clone(), ("rust-v0.1.0".into(), "rust-v0.3.0-alpha.1".into())]
				);
			} else {
				assert_eq!(
					pairs, fresh,
					"foreign or unidentified history must not restrict candidates"
				);
			}

			assert_eq!(select(1), pairs[..1]);
			assert_eq!(select(2), pairs[..pairs.len().min(2)]);
			assert_eq!(select(10), pairs);
		}

		fs::write(&out, "{broken").unwrap();

		assert_eq!(select(0), fresh);
	}
}
