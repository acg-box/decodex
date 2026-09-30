use std::{collections::BTreeSet, env, fs, path::Path, process};

use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::{
	RELEASE_DELTA_SCHEMA, RadarBackfillReleaseRangeRequest, SIGNAL_SCHEMA,
	prelude::{Result, eyre},
	release_delta::backfill::{
		execution,
		model::{PreparedReleaseDelta, ReleaseSelection},
	},
};

pub(in crate::release_delta::backfill) fn selected_release_comparison(
	payload: &Value,
	stable_tag: Option<&str>,
	preview_tag: Option<&str>,
) -> Result<ReleaseSelection> {
	crate::validate_expected_schema(payload, RELEASE_DELTA_SCHEMA, "Release-delta")?;

	let entry =
		payload.as_object().ok_or_else(|| eyre::eyre!("Release-delta must be an object"))?;
	let target_stable = stable_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("stable_release")))
		.ok_or_else(|| eyre::eyre!("stable release tag could not be selected"))?;
	let target_preview = preview_tag
		.map(str::to_owned)
		.or_else(|| release_delta_release_tag(entry.get("prerelease")))
		.ok_or_else(|| eyre::eyre!("preview release tag could not be selected"))?;
	let comparisons = entry
		.get("comparisons")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Release-delta comparisons must be a list"))?;

	for comparison in comparisons {
		let Some(comparison) = comparison.as_object() else {
			continue;
		};

		if crate::string_field(comparison, "stable_tag_name") == Some(target_stable.as_str())
			&& crate::string_field(comparison, "prerelease_tag_name")
				== Some(target_preview.as_str())
		{
			return Ok(ReleaseSelection {
				stable_tag: target_stable,
				preview_tag: target_preview,
				pr_numbers: comparison_pr_numbers(comparison),
			});
		}
	}

	Err(eyre::eyre!("No comparison found for {target_stable} -> {target_preview}"))
}

pub(in crate::release_delta::backfill) fn published_pr_numbers(
	signals_dir: &Path,
	repo: &str,
) -> Result<BTreeSet<u64>> {
	let mut published = BTreeSet::new();
	let pr_prefix = format!("https://github.com/{repo}/pull/");

	for path in crate::sorted_json_files(signals_dir)? {
		let payload = crate::load_json(&path)?;

		crate::validate_expected_schema(&payload, SIGNAL_SCHEMA, "Signal")?;

		if payload.pointer("/source_refs/repo").and_then(Value::as_str) != Some(repo) {
			continue;
		}

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(Value::as_object)
			.and_then(|refs| crate::string_field(refs, "pr_url"))
			.and_then(|url| url.strip_prefix(&pr_prefix))
			.filter(|number| !number.is_empty() && number.bytes().all(|ch| ch.is_ascii_digit()))
			.and_then(|number| number.parse::<u64>().ok())
		{
			published.insert(pr_number);
		}
	}

	Ok(published)
}

pub(in crate::release_delta::backfill) fn prepare_release_delta_path(
	request: &RadarBackfillReleaseRangeRequest,
	root: &Path,
) -> Result<PreparedReleaseDelta> {
	if !request.refresh_release_delta_first {
		return Ok(PreparedReleaseDelta {
			path: crate::resolve_against(root, &request.release_delta),
			cleanup_dir: None,
		});
	}

	let temp_root = env::temp_dir().join(format!(
		"decodex-prerelease-delta-{}-{}",
		process::id(),
		OffsetDateTime::now_utc().unix_timestamp_nanos()
	));

	fs::create_dir_all(&temp_root)?;

	let release_delta = temp_root.join("release-delta.json");

	execution::run_refresh_release_delta(request, &release_delta, true)?;

	Ok(PreparedReleaseDelta { path: release_delta, cleanup_dir: Some(temp_root) })
}

fn release_delta_release_tag(value: Option<&Value>) -> Option<String> {
	value
		.and_then(Value::as_object)
		.and_then(|release| crate::string_field(release, "tag_name"))
		.filter(|tag| !tag.is_empty())
		.map(str::to_owned)
}

fn comparison_pr_numbers(comparison: &Map<String, Value>) -> Vec<u64> {
	comparison
		.get("compare")
		.and_then(Value::as_object)
		.and_then(|compare| compare.get("pr_numbers"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_u64)
		.collect()
}
