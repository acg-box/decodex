use std::{collections::HashSet, path::Path};

use serde_json::Value;

use crate::prelude::Result;

pub(super) fn published_subjects(
	signals_dir: &Path,
	repo: &str,
) -> Result<(HashSet<u64>, HashSet<String>)> {
	let mut published_prs = HashSet::new();
	let mut published_shas = HashSet::new();

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
			.and_then(|url| crate::extract_pr_number_from_url(url, repo))
		{
			published_prs.insert(pr_number);
		}

		for url in crate::string_array(payload.pointer("/source_refs/commit_urls")) {
			if let Some(sha) = crate::extract_commit_sha_from_url(&url, repo) {
				published_shas.insert(sha);
			}
		}
	}

	Ok((published_prs, published_shas))
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use crate::{review_queue::published, tests::fixtures};

	#[test]
	fn published_pr_references_require_positive_decimal_numbers() {
		let temp = tempfile::tempdir().unwrap();
		let mut signal = fixtures::valid_signal();

		for suffix in ["0", "00", "+1", "-1", "1?query", "18446744073709551616"] {
			signal["source_refs"]["pr_url"] =
				serde_json::json!(format!("https://github.com/openai/codex/pull/{suffix}"));

			crate::write_json(&temp.path().join("signal.json"), &signal).unwrap();

			let (prs, _) = published::published_subjects(temp.path(), "openai/codex").unwrap();

			assert!(prs.is_empty(), "invalid PR reference: {suffix}");
		}
	}

	#[test]
	fn foreign_signals_do_not_suppress_the_requested_repository() {
		let temp = tempfile::tempdir().unwrap();
		let mut foreign = fixtures::valid_signal();

		foreign["source_refs"]["repo"] = serde_json::json!("other/project");
		foreign["source_refs"]["pr_url"] =
			serde_json::json!("https://github.com/other/project/pull/22414");
		foreign["source_refs"]["commit_urls"] = serde_json::json!([
			"https://github.com/other/project/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
		]);

		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &foreign).unwrap();

		let (prs, commits) = published::published_subjects(temp.path(), "openai/codex").unwrap();

		assert!(prs.is_empty() && commits.is_empty());

		foreign["source_refs"]["repo"] = serde_json::json!("openai/codex");

		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &foreign).unwrap();

		let (prs, commits) = published::published_subjects(temp.path(), "openai/codex").unwrap();

		assert!(prs.is_empty() && commits.is_empty());

		let mut matching = fixtures::valid_signal();

		matching["source_refs"]["commit_urls"] = serde_json::json!([
			"https://github.com/openai/codex/commit/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"https://example.com/openai/codex/commit/cccccccccccccccccccccccccccccccccccccccc"
		]);

		crate::write_json(&temp.path().join("openai-codex-pr-22414.json"), &matching).unwrap();

		let (prs, commits) = published::published_subjects(temp.path(), "openai/codex").unwrap();

		assert_eq!(prs, HashSet::from([22_414]));
		assert_eq!(commits, HashSet::from(["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()]));
	}
}
