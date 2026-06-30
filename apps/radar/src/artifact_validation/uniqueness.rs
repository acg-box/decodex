//! Cross-artifact uniqueness checks.

use std::path::Path;

use serde_json::Value;

use super::model::ValidationState;

pub(crate) fn validate_signal_slug_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	let Some(slug) = payload.get("slug").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) = state.seen_signal_slugs.insert(slug.to_owned(), path.to_path_buf()) {
		errors.push(format!(
			"{}: duplicate slug {slug:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
}
