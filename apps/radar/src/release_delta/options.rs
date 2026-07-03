//! Release option filtering and compact response shaping.

use crate::{
	prelude::Result,
	release_delta::{
		self, BTreeSet, Path, RadarRefreshReleaseDeltaReport, Value, eyre, optional_value_string,
		required_value_string, serde_json,
	},
};

pub(super) fn filter_release_options(
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

pub(super) fn compact_releases(releases: &[Value]) -> Result<Vec<Value>> {
	releases.iter().map(compact_release).collect()
}

pub(super) fn compact_release(release: &Value) -> Result<Value> {
	let tag_name = required_release_tag(release)?;

	Ok(serde_json::json!({
		"tag_name": tag_name,
		"name": optional_value_string(release, "name").unwrap_or_else(|| tag_name.to_owned()),
		"prerelease": release.get("prerelease").and_then(Value::as_bool).unwrap_or(false),
		"published_at": required_value_string(release, "published_at")?,
		"url": required_value_string(release, "html_url")?,
	}))
}

pub(super) fn stable_version_key(tag_name: &str, tag_prefix: &str) -> Vec<u64> {
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

pub(super) fn release_sort_key(release: &Value) -> &str {
	release.get("published_at").and_then(Value::as_str).unwrap_or_default()
}

pub(super) fn required_release_tag(release: &Value) -> Result<&str> {
	release_tag(release).ok_or_else(|| eyre::eyre!("Release payload is missing tag_name"))
}

pub(super) fn release_tag(release: &Value) -> Option<&str> {
	release.get("tag_name").and_then(Value::as_str)
}

pub(super) fn release_delta_report(
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
		out: release_delta::absolute_repo_path(root, out),
	}
}
