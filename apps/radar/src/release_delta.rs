//! Release delta artifact generation and release-window backfill orchestration.

mod backfill;

use super::{
	BTreeMap, BTreeSet, GitHubApi, HashSet, Path, RELEASE_DELTA_SCHEMA,
	RadarRefreshReleaseDeltaReport, RadarRefreshReleaseDeltaRequest, RefreshKind, Value,
	absolute_repo_path, extract_commit_sha_from_url, extract_pr_number_from_url, eyre,
	github_token, iter, load_json, optional_value_string, pretty_json, repo_root,
	required_value_i64, required_value_string, serde_json, sorted_json_files, string_array,
	string_array_from_value, utc_now_iso, validate_artifact_errors, validate_signal_file,
	write_json_if_material_changed,
};

pub(crate) use self::backfill::backfill_release_range;

#[derive(Clone, Debug)]
struct ReleasePair {
	stable: Value,
	preview: Value,
}

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

fn build_release_delta(
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

fn select_release(
	releases: &[Value],
	tag_prefix: &str,
	prerelease: bool,
) -> crate::prelude::Result<Value> {
	releases
		.iter()
		.find(|release| {
			!release.get("draft").and_then(Value::as_bool).unwrap_or(false)
				&& release_tag(release).is_some_and(|tag| tag.starts_with(tag_prefix))
				&& release.get("prerelease").and_then(Value::as_bool).unwrap_or(false) == prerelease
		})
		.cloned()
		.ok_or_else(|| {
			let kind = if prerelease { "prerelease" } else { "stable release" };

			eyre::eyre!("No {kind} found for tag prefix {tag_prefix:?}")
		})
}

fn select_release_options(
	request: &RadarRefreshReleaseDeltaRequest,
	releases: &[Value],
) -> crate::prelude::Result<(Vec<Value>, Vec<Value>)> {
	let min_stable_key = stable_version_key(&request.min_stable_tag, &request.tag_prefix);
	let mut stable = relevant_releases(releases, &request.tag_prefix)
		.into_iter()
		.filter(|release| {
			!release.get("prerelease").and_then(Value::as_bool).unwrap_or(false)
				&& release_tag(release).is_some_and(|tag| {
					stable_version_key(tag, &request.tag_prefix) >= min_stable_key
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
				&& release_tag(release).is_some_and(|tag| tag.starts_with(tag_prefix))
		})
		.cloned()
		.collect()
}

fn select_release_pairs(
	request: &RadarRefreshReleaseDeltaRequest,
	root: &Path,
	stable_release: &Value,
	prerelease: &Value,
	stable_releases: &[Value],
	preview_releases: &[Value],
) -> crate::prelude::Result<Vec<ReleasePair>> {
	let default_pair = ReleasePair { stable: stable_release.clone(), preview: prerelease.clone() };
	let releases_by_tag = stable_releases
		.iter()
		.chain(preview_releases)
		.filter_map(|release| release_tag(release).map(|tag| (tag.to_owned(), release.clone())))
		.collect::<BTreeMap<_, _>>();
	let previous_pairs = previous_signal_pairs(&absolute_repo_path(root, &request.out))?
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
				.filter(move |preview| release_sort_key(preview) > release_sort_key(stable))
				.map(move |preview| ReleasePair {
					stable: stable.clone(),
					preview: preview.clone(),
				})
		})
		.collect::<Vec<_>>();

	candidates.sort_by(|left, right| {
		(release_sort_key(&right.preview), release_sort_key(&right.stable))
			.cmp(&(release_sort_key(&left.preview), release_sort_key(&left.stable)))
	});

	candidates
}

fn unique_release_pairs(pairs: Vec<ReleasePair>) -> Vec<ReleasePair> {
	let mut seen = BTreeSet::new();
	let mut unique = Vec::new();

	for pair in pairs {
		let Some(stable_tag) = release_tag(&pair.stable) else {
			continue;
		};
		let Some(preview_tag) = release_tag(&pair.preview) else {
			continue;
		};
		let key = (stable_tag.to_owned(), preview_tag.to_owned());

		if seen.insert(key) {
			unique.push(pair);
		}
	}

	unique
}

fn previous_signal_pairs(path: &Path) -> crate::prelude::Result<Vec<(String, String)>> {
	if !path.exists() {
		return Ok(Vec::new());
	}

	let Ok(previous) = load_json(path) else {
		return Ok(Vec::new());
	};
	let mut keys = Vec::new();
	let mut seen = BTreeSet::new();

	for comparison in previous.get("comparisons").and_then(Value::as_array).into_iter().flatten() {
		if string_array(comparison.get("tracked_signal_slugs")).is_empty() {
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

fn build_release_comparison(
	api: &GitHubApi,
	request: &RadarRefreshReleaseDeltaRequest,
	pair: &ReleasePair,
	signals: &[Value],
) -> crate::prelude::Result<Value> {
	let stable_tag = required_release_tag(&pair.stable)?;
	let preview_tag = required_release_tag(&pair.preview)?;
	let compare = api
		.get(&format!(
			"https://api.github.com/repos/{}/compare/{stable_tag}...{preview_tag}",
			request.repo
		))?
		.payload;
	let commits = compare
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Expected compare.commits from GitHub API"))?;
	let commit_shas = commits
		.iter()
		.filter_map(|commit| commit.get("sha").and_then(Value::as_str).map(str::to_owned))
		.collect::<Vec<_>>();
	let pr_numbers = compare_pr_numbers(commits);
	let tracked_signal_slugs = tracked_signal_slugs(signals, &commit_shas, &pr_numbers);

	Ok(serde_json::json!({
		"stable_tag_name": stable_tag,
		"prerelease_tag_name": preview_tag,
		"compare": {
			"status": required_value_string(&compare, "status")?,
			"ahead_by": required_value_i64(&compare, "ahead_by")?,
			"total_commits": required_value_i64(&compare, "total_commits")?,
			"url": required_value_string(&compare, "html_url")?,
			"commit_shas": commit_shas,
			"pr_numbers": pr_numbers,
		},
		"tracked_signal_slugs": tracked_signal_slugs,
	}))
}

fn load_signal_entries(signals_dir: &Path, repo: &str) -> crate::prelude::Result<Vec<Value>> {
	let mut entries = Vec::new();

	for path in sorted_json_files(signals_dir)? {
		let payload = load_json(&path)?;

		validate_signal_file(&path, &payload)?;

		if payload.pointer("/source_refs/repo").and_then(Value::as_str) == Some(repo) {
			entries.push(payload);
		}
	}

	Ok(entries)
}

fn tracked_signal_slugs(
	signals: &[Value],
	commit_shas: &[String],
	pr_numbers: &[u64],
) -> Vec<String> {
	let commit_set = commit_shas.iter().map(String::as_str).collect::<HashSet<_>>();
	let pr_set = pr_numbers.iter().copied().collect::<HashSet<_>>();
	let mut sorted_signals = signals.iter().collect::<Vec<_>>();

	sorted_signals.sort_by(|left, right| {
		right
			.get("published_at")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.cmp(left.get("published_at").and_then(Value::as_str).unwrap_or_default())
	});

	sorted_signals
		.into_iter()
		.filter(|signal| {
			let signal_shas = signal_commit_shas(signal);
			let signal_pr = signal_pr_number(signal);

			signal_shas.iter().any(|sha| commit_set.contains(sha.as_str()))
				|| signal_pr.is_some_and(|number| pr_set.contains(&number))
		})
		.filter_map(|signal| signal.get("slug").and_then(Value::as_str).map(str::to_owned))
		.collect()
}

fn signal_commit_shas(signal: &Value) -> Vec<String> {
	string_array(signal.pointer("/source_refs/commit_urls"))
		.into_iter()
		.filter_map(|url| extract_commit_sha_from_url(&url))
		.collect()
}

fn signal_pr_number(signal: &Value) -> Option<u64> {
	signal
		.pointer("/source_refs/pr_url")
		.and_then(Value::as_str)
		.and_then(extract_pr_number_from_url)
}

fn compare_pr_numbers(commits: &[Value]) -> Vec<u64> {
	let mut numbers = commits
		.iter()
		.flat_map(|commit| {
			commit
				.pointer("/commit/message")
				.and_then(Value::as_str)
				.map(pr_numbers_from_message)
				.unwrap_or_default()
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	numbers.sort();

	numbers
}

fn pr_numbers_from_message(message: &str) -> Vec<u64> {
	let mut numbers = Vec::new();
	let mut rest = message;

	while let Some(start) = rest.find("(#") {
		let candidate = &rest[start + 2..];
		let Some(end) = candidate.find(')') else {
			break;
		};
		let digits = &candidate[..end];

		if !digits.is_empty()
			&& digits.chars().all(|ch| ch.is_ascii_digit())
			&& let Ok(number) = digits.parse::<u64>()
		{
			numbers.push(number);
		}

		rest = &candidate[end + 1..];
	}

	numbers
}

fn filter_release_options(
	stable_releases: &[Value],
	preview_releases: &[Value],
	comparison_entries: &[Value],
) -> (Vec<Value>, Vec<Value>) {
	let allowed_stable_tags = comparison_entries
		.iter()
		.filter_map(|entry| entry.get("stable_tag_name").and_then(Value::as_str))
		.collect::<BTreeSet<_>>();
	let allowed_preview_tags = comparison_entries
		.iter()
		.filter_map(|entry| entry.get("prerelease_tag_name").and_then(Value::as_str))
		.collect::<BTreeSet<_>>();
	let stable = stable_releases
		.iter()
		.filter(|release| release_tag(release).is_some_and(|tag| allowed_stable_tags.contains(tag)))
		.cloned()
		.collect();
	let preview = preview_releases
		.iter()
		.filter(|release| {
			release_tag(release).is_some_and(|tag| allowed_preview_tags.contains(tag))
		})
		.cloned()
		.collect();

	(stable, preview)
}

fn compact_releases(releases: &[Value]) -> crate::prelude::Result<Vec<Value>> {
	releases.iter().map(compact_release).collect()
}

fn compact_release(release: &Value) -> crate::prelude::Result<Value> {
	let tag_name = required_release_tag(release)?;

	Ok(serde_json::json!({
		"tag_name": tag_name,
		"name": optional_value_string(release, "name").unwrap_or_else(|| tag_name.to_owned()),
		"prerelease": release.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
		"published_at": required_value_string(release, "published_at")?,
		"url": required_value_string(release, "html_url")?,
	}))
}

fn stable_version_key(tag_name: &str, tag_prefix: &str) -> Vec<u64> {
	tag_name
		.strip_prefix(tag_prefix)
		.unwrap_or(tag_name)
		.split('.')
		.map(|part| {
			let digits = part.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>();

			digits.parse::<u64>().unwrap_or(0)
		})
		.collect()
}

fn release_sort_key(release: &Value) -> &str {
	release.get("published_at").and_then(Value::as_str).unwrap_or_default()
}

fn required_release_tag(release: &Value) -> crate::prelude::Result<&str> {
	release_tag(release).ok_or_else(|| eyre::eyre!("Release payload is missing tag_name"))
}

fn release_tag(release: &Value) -> Option<&str> {
	release.get("tag_name").and_then(Value::as_str)
}

fn release_delta_report(
	payload: &Value,
	changed: bool,
	root: &Path,
	out: &Path,
) -> RadarRefreshReleaseDeltaReport {
	RadarRefreshReleaseDeltaReport {
		changed,
		stable_tag_name: payload
			.pointer("/stable_release/tag_name")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_owned(),
		prerelease_tag_name: payload
			.pointer("/prerelease/tag_name")
			.and_then(Value::as_str)
			.unwrap_or_default()
			.to_owned(),
		comparisons: payload.get("comparisons").and_then(Value::as_array).map_or(0, Vec::len),
		out: absolute_repo_path(root, out),
	}
}
