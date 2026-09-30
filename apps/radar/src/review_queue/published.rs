use std::{collections::HashSet, path::Path};

use serde_json::Value;

use crate::prelude::Result;

pub(super) fn published_subjects(
	signals_dir: &Path,
	repo: &str,
) -> Result<(HashSet<u64>, HashSet<String>)> {
	let mut published_prs = HashSet::new();
	let mut published_shas = HashSet::new();
	let pr_prefix = format!("https://github.com/{repo}/pull/");
	let commit_prefix = format!("https://github.com/{repo}/commit/");

	for path in crate::sorted_json_files(signals_dir)? {
		let payload = crate::load_json(&path)?;

		crate::validate_signal_file(&path, &payload)?;

		if payload.pointer("/source_refs/repo").and_then(Value::as_str) != Some(repo) {
			continue;
		}

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(|refs| refs.get("pr_url"))
			.and_then(Value::as_str)
			.and_then(|url| url.strip_prefix(&pr_prefix))
			.filter(|number| !number.is_empty() && number.bytes().all(|ch| ch.is_ascii_digit()))
			.and_then(|number| number.parse::<u64>().ok())
		{
			published_prs.insert(pr_number);
		}

		for url in crate::string_array(payload.pointer("/source_refs/commit_urls")) {
			if let Some(sha) = url.strip_prefix(&commit_prefix)
				&& (7..=40).contains(&sha.len())
				&& sha.bytes().all(|ch| ch.is_ascii_hexdigit())
			{
				published_shas.insert(sha.to_owned());
			}
		}
	}

	Ok((published_prs, published_shas))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn foreign_signals_do_not_suppress_the_requested_repository() {
		let temp = tempfile::tempdir().unwrap();
		let mut foreign = crate::tests::fixtures::valid_signal();
		foreign["source_refs"]["repo"] = serde_json::json!("other/project");
		foreign["source_refs"]["pr_url"] =
			serde_json::json!("https://github.com/other/project/pull/22414");
		foreign["source_refs"]["commit_urls"] = serde_json::json!([
			"https://github.com/other/project/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		]);
		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &foreign).unwrap();
		let (prs, commits) = published_subjects(temp.path(), "openai/codex").unwrap();
		assert!(prs.is_empty() && commits.is_empty());

		foreign["source_refs"]["repo"] = serde_json::json!("openai/codex");
		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &foreign).unwrap();
		let (prs, commits) = published_subjects(temp.path(), "openai/codex").unwrap();
		assert!(prs.is_empty() && commits.is_empty());

		let mut matching = crate::tests::fixtures::valid_signal();
		matching["source_refs"]["commit_urls"] = serde_json::json!([
			"https://github.com/openai/codex/commit/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"https://example.com/openai/codex/commit/cccccccccccccccccccccccccccccccccccccccc"
		]);
		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &matching).unwrap();
		let (prs, commits) = published_subjects(temp.path(), "openai/codex").unwrap();
		assert_eq!(prs, HashSet::from([22414]));
		assert_eq!(commits, HashSet::from(["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()]));
	}
}
