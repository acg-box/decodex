use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::{
	GENERIC_COMMIT_TITLES,
	prelude::{Result, eyre},
};

pub(super) fn rendered_source_refs(bundle: &Map<String, Value>) -> Result<Value> {
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

pub(super) fn bundle_commits(bundle: &Map<String, Value>) -> Result<Vec<&Map<String, Value>>> {
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
