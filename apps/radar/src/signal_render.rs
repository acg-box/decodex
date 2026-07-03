//! Signal artifact rendering helpers for Radar.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::{
	GENERIC_COMMIT_TITLES, SIGNAL_SCHEMA,
	prelude::{Result, eyre},
};

pub(super) fn rendered_signal(
	bundle: &Value,
	analysis: &Value,
	published_at_override: Option<&str>,
	config_flags: Vec<String>,
) -> Result<Value> {
	let bundle = crate::object_value(bundle, "Bundle")?;
	let analysis = crate::object_value(analysis, "Analysis draft")?;
	let title = crate::required_string(analysis, "title", "analysis draft")?;
	let slug = crate::string_field(analysis, "slug")
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(|| crate::slugify(title));
	let mut signal = Map::new();

	signal.insert("schema".into(), serde_json::json!(SIGNAL_SCHEMA));
	signal.insert("slug".into(), serde_json::json!(slug));
	signal.insert("lane".into(), serde_json::json!("github"));

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		signal.insert(
			field.into(),
			serde_json::json!(crate::required_string(analysis, field, "analysis draft")?),
		);
	}

	signal.insert(
		"published_at".into(),
		serde_json::json!(pick_published_at(bundle, analysis, published_at_override)?),
	);
	signal.insert("config_flags".into(), serde_json::json!(config_flags));
	signal.insert(
		"proof_points".into(),
		analysis
			.get("proof_points")
			.cloned()
			.ok_or_else(|| eyre::eyre!("analysis draft proof_points is required"))?,
	);
	signal.insert("source_refs".into(), rendered_source_refs(bundle)?);

	for field in ["how_to_try", "expected_effect", "caveats", "watch_state"] {
		if crate::is_truthy_json_value(analysis.get(field)) {
			signal.insert(
				field.into(),
				analysis
					.get(field)
					.cloned()
					.ok_or_else(|| eyre::eyre!("analysis draft {field} is required"))?,
			);
		}
	}

	Ok(Value::Object(signal))
}

pub(super) fn rendered_config_flags(
	bundle: &Value,
	analysis: &Value,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let raw_flags = analysis
		.get("config_flags")
		.filter(|value| !value.is_null())
		.or_else(|| bundle.get("extracted_flags"));

	normalized_config_flags(raw_flags, known_features)
}

fn normalized_config_flags(
	raw_flags: Option<&Value>,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let Some(raw_flags) = raw_flags.and_then(Value::as_array) else {
		return Vec::new();
	};
	let mut normalized = Vec::new();
	let mut seen = BTreeSet::new();

	for flag in raw_flags {
		let Some(raw_value) = flag.as_str() else {
			continue;
		};
		let mut value = raw_value.trim().to_owned();

		if value.is_empty() || seen.contains(&value) {
			continue;
		}

		if let Some(feature_name) = normalize_feature_flag(&value, known_features) {
			value = format!("features.{feature_name} = true");
		}

		if !(value.starts_with("--")
			|| value.contains('=')
			|| value.ends_with(".json")
			|| value.ends_with(".toml"))
		{
			continue;
		}

		seen.insert(value.clone());
		normalized.push(value);
	}

	normalized
}

fn normalize_feature_flag(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let lower = value.to_ascii_lowercase();

	if let Some(candidate) = lower.strip_prefix("--enable ") {
		let candidate = candidate.trim();

		return known_feature_name(candidate, known_features);
	}

	let candidate = lower.strip_prefix("features.").unwrap_or(&lower);
	let (name, enabled) =
		candidate.split_once('=').map_or((candidate.trim(), true), |(name, enabled)| {
			(name.trim(), enabled.trim() == "true")
		});

	enabled.then(|| known_feature_name(name, known_features)).flatten()
}

fn known_feature_name(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let valid = !value.is_empty()
		&& value.chars().all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		});

	if valid && known_features.contains(value) { Some(value.to_owned()) } else { None }
}

fn rendered_source_refs(bundle: &Map<String, Value>) -> Result<Value> {
	let commits = bundle_commits(bundle)?;
	let commit_urls = commits
		.iter()
		.filter_map(|commit| commit.get("url").and_then(Value::as_str))
		.collect::<Vec<_>>();
	let mut refs = Map::new();

	refs.insert(
		"repo".into(),
		serde_json::json!(crate::required_string(bundle, "repo", "bundle")?),
	);
	refs.insert("commit_urls".into(), serde_json::json!(commit_urls));
	refs.insert("items".into(), serde_json::json!(rendered_source_items(bundle)?));

	if let Some(pr_url) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| crate::string_field(primary_pr, "url"))
	{
		refs.insert("pr_url".into(), serde_json::json!(pr_url));
	}

	Ok(Value::Object(refs))
}

fn rendered_source_items(bundle: &Map<String, Value>) -> Result<Vec<Map<String, Value>>> {
	let mut items = Vec::new();

	if let Some(primary_pr) = bundle.get("primary_pr").and_then(Value::as_object)
		&& let (Some(url), Some(title)) =
			(crate::string_field(primary_pr, "url"), crate::string_field(primary_pr, "title"))
	{
		let mut item = Map::new();

		item.insert("kind".into(), serde_json::json!("pull_request"));
		item.insert("title".into(), serde_json::json!(crate::first_line(title)));
		item.insert("url".into(), serde_json::json!(url));

		if let Some(number) = primary_pr.get("number").and_then(Value::as_i64) {
			item.insert("meta".into(), serde_json::json!(format!("#{number}")));
		}

		items.push(item);
	}

	items.extend(rendered_commit_items(bundle)?);

	Ok(items)
}

fn rendered_commit_items(bundle: &Map<String, Value>) -> Result<Vec<Map<String, Value>>> {
	let mut fallback_items = Vec::new();
	let mut picked_items = Vec::new();
	let mut seen_titles = BTreeSet::new();

	for commit in bundle_commits(bundle)? {
		let title =
			crate::first_line(commit.get("message").and_then(Value::as_str).unwrap_or_default());

		if title.is_empty()
			|| !seen_titles.insert(title.clone())
			|| title.starts_with("Merge branch ")
		{
			continue;
		}

		let entry = rendered_commit_item(commit, &title)?;

		fallback_items.push(entry.clone());

		if !GENERIC_COMMIT_TITLES.contains(&title.to_ascii_lowercase().as_str()) {
			picked_items.push(entry);
		}
	}

	Ok(if picked_items.is_empty() { fallback_items } else { picked_items })
}

fn rendered_commit_item(commit: &Map<String, Value>, title: &str) -> Result<Map<String, Value>> {
	let sha = crate::required_string(commit, "sha", "bundle commit")?;
	let mut entry = Map::new();

	entry.insert("kind".into(), serde_json::json!("commit"));
	entry.insert("title".into(), serde_json::json!(title));
	entry.insert(
		"url".into(),
		serde_json::json!(crate::required_string(commit, "url", "bundle commit")?),
	);
	entry.insert("meta".into(), serde_json::json!(crate::short_sha(sha)));

	Ok(entry)
}

fn bundle_commits(bundle: &Map<String, Value>) -> Result<Vec<&Map<String, Value>>> {
	bundle
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Bundle commits must be a list"))?
		.iter()
		.map(|commit| {
			commit.as_object().ok_or_else(|| eyre::eyre!("Bundle commit must be an object"))
		})
		.collect()
}

fn pick_published_at(
	bundle: &Map<String, Value>,
	analysis: &Map<String, Value>,
	override_value: Option<&str>,
) -> Result<String> {
	if let Some(value) = override_value.filter(|value| !value.is_empty()) {
		return Ok(value.to_owned());
	}
	if let Some(value) =
		crate::string_field(analysis, "published_at").filter(|value| !value.is_empty())
	{
		return Ok(value.to_owned());
	}
	if let Some(value) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| crate::string_field(primary_pr, "merged_at"))
		.filter(|value| !value.is_empty())
	{
		return Ok(value.to_owned());
	}

	let first_commit = bundle_commits(bundle)?
		.into_iter()
		.next()
		.ok_or_else(|| eyre::eyre!("Bundle commits must be a non-empty list"))?;

	if let Some(value) =
		crate::string_field(first_commit, "committed_at").filter(|value| !value.is_empty())
	{
		Ok(value.to_owned())
	} else {
		crate::utc_now_iso()
	}
}
