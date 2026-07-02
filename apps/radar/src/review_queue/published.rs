use std::{collections::HashSet, path::Path};

use serde_json::Value;

use crate::prelude::Result;

pub(super) fn published_subjects(signals_dir: &Path) -> Result<(HashSet<u64>, HashSet<String>)> {
	let mut published_prs = HashSet::new();
	let mut published_shas = HashSet::new();

	for path in crate::sorted_json_files(signals_dir)? {
		let payload = crate::load_json(&path)?;

		crate::validate_signal_file(&path, &payload)?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(|refs| refs.get("pr_url"))
			.and_then(Value::as_str)
			.and_then(crate::extract_pr_number_from_url)
		{
			published_prs.insert(pr_number);
		}

		for url in crate::string_array(payload.pointer("/source_refs/commit_urls")) {
			if let Some(sha) = crate::extract_commit_sha_from_url(&url) {
				published_shas.insert(sha);
			}
		}
	}

	Ok((published_prs, published_shas))
}
