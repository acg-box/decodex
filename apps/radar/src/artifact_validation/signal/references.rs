use serde_json::Value;

use crate::artifact_validation::{constants::SOURCE_ITEM_KINDS, support};

pub(super) fn validate_signal_source_refs(refs: Option<&Value>, errors: &mut Vec<String>) {
	let Some(refs) = refs.and_then(Value::as_object) else {
		errors.push("source_refs must be an object".into());

		return;
	};

	if support::string_field(refs, "repo").is_none_or(|repo| !repo.contains('/')) {
		errors.push("source_refs.repo must be owner/name".into());
	}

	validate_signal_source_items(refs.get("items"), errors);

	let pr_url = refs.get("pr_url");
	let commit_urls = refs.get("commit_urls");
	let items = refs.get("items");

	if pr_url.is_none()
		&& support::is_empty_or_missing_array(commit_urls)
		&& support::is_empty_or_missing_array(items)
	{
		errors.push("source_refs must include pr_url, commit URLs, or source_refs.items".into());
	}
	if pr_url.is_some_and(|url| !support::is_https_string(Some(url))) {
		errors.push("source_refs.pr_url must be an https URL when present".into());
	}
	if commit_urls.is_some_and(|urls| !support::is_https_string_array(urls)) {
		errors.push("source_refs.commit_urls must be a list of https URLs".into());
	}
}

fn validate_signal_source_items(items: Option<&Value>, errors: &mut Vec<String>) {
	let Some(items) = items else {
		return;
	};

	if items.as_array().is_some_and(Vec::is_empty) {
		return;
	}

	let valid = items.as_array().is_some_and(|items| {
		items.iter().all(|item| {
			item.as_object().is_some_and(|item| {
				support::matches_one_of(item.get("kind"), SOURCE_ITEM_KINDS)
					&& support::is_non_empty_string(item.get("title"))
					&& support::is_https_string(item.get("url"))
					&& item.get("meta").is_none_or(|meta| meta.as_str().is_some())
			})
		})
	});

	if !valid {
		errors.push("source_refs.items must be a list of titled source entries".into());
	}
}
