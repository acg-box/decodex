//! Cross-file social artifact uniqueness checks.

use std::{collections::BTreeMap, path::Path};

use serde_json::Value;

use super::{SOCIAL_POST_SCHEMA, SOCIAL_PUBLISH_RESERVATION_SCHEMA};
use crate::{path_arg, repo_root};

#[derive(Debug)]
pub(crate) struct SocialValidationState {
	active_reservation_idempotency_keys: BTreeMap<String, String>,
	terminal_post_idempotency_keys: BTreeMap<String, String>,
}
impl SocialValidationState {
	pub(crate) fn new() -> Self {
		Self {
			active_reservation_idempotency_keys: BTreeMap::new(),
			terminal_post_idempotency_keys: BTreeMap::new(),
		}
	}
}

pub(crate) fn validate_social_cross_file_constraints(
	path: &Path,
	payload: &Value,
	state: &mut SocialValidationState,
	errors: &mut Vec<String>,
) {
	let root = repo_root().ok();
	let display_path = root
		.as_deref()
		.map_or_else(|| path.to_string_lossy().replace('\\', "/"), |root| path_arg(root, path));

	match payload.get("schema").and_then(Value::as_str) {
		Some(SOCIAL_POST_SCHEMA) => {
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
				state.terminal_post_idempotency_keys.insert(key.to_owned(), display_path.clone())
			{
				errors.push(format!(
					"{display_path}: duplicate terminal social_post idempotency_key {key:?} also used by {existing}"
				));
			}
			if let Some(existing) = state.active_reservation_idempotency_keys.get(key) {
				errors.push(format!(
					"{display_path}: terminal social_post idempotency_key {key:?} conflicts with active reservation {existing}"
				));
			}
		},
		Some(SOCIAL_PUBLISH_RESERVATION_SCHEMA) => {
			if payload.get("status").and_then(Value::as_str) != Some("active") {
				return;
			}

			let Some(key) = payload.get("idempotency_key").and_then(Value::as_str) else {
				return;
			};

			if let Some(existing) = state.terminal_post_idempotency_keys.get(key) {
				errors.push(format!(
					"{display_path}: active social_publish_reservation idempotency_key {key:?} conflicts with terminal social_post {existing}"
				));
			}
			if let Some(existing) = state
				.active_reservation_idempotency_keys
				.insert(key.to_owned(), display_path.clone())
			{
				errors.push(format!(
					"{display_path}: duplicate active social_publish_reservation idempotency_key {key:?} also used by {existing}"
				));
			}
		},
		_ => {},
	}
}
