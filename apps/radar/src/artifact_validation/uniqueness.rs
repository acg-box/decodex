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

pub(crate) fn validate_terminal_social_post_idempotency_key_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	let status = payload.get("status").and_then(Value::as_str);

	if !matches!(status, Some("published" | "blocked")) {
		return;
	}

	let Some(key) = payload
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("idempotency_key"))
		.and_then(Value::as_str)
	else {
		return;
	};

	if let Some(existing) =
		state.seen_terminal_social_post_idempotency_keys.insert(key.to_owned(), path.to_path_buf())
	{
		errors.push(format!(
			"{}: duplicate terminal social_post idempotency_key {key:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
	if let Some(existing) = state.active_social_publish_reservation_idempotency_keys.get(key) {
		errors.push(format!(
			"{}: terminal social_post idempotency_key {key:?} conflicts with active reservation {}",
			path.display(),
			existing.display()
		));
	}
}

pub(crate) fn validate_active_social_publish_reservation_uniqueness(
	path: &Path,
	payload: &Value,
	state: &mut ValidationState,
	errors: &mut Vec<String>,
) {
	if payload.get("status").and_then(Value::as_str) != Some("active") {
		return;
	}

	let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
		return;
	};

	if let Some(existing) = state.seen_terminal_social_post_idempotency_keys.get(key) {
		errors.push(format!(
			"{}: active social_publish_reservation idempotency_key {key:?} conflicts with terminal social_post {}",
			path.display(),
			existing.display()
		));
	}
	if let Some(existing) = state
		.active_social_publish_reservation_idempotency_keys
		.insert(key.to_owned(), path.to_path_buf())
	{
		errors.push(format!(
			"{}: duplicate active social_publish_reservation idempotency_key {key:?} also used by {}",
			path.display(),
			existing.display()
		));
	}
}
