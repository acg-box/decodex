//! Text, path, and JSON value normalization helpers.

use std::{
	env,
	path::{Path, PathBuf},
};

use serde_json::Value;

use crate::{
	paths::DEFAULT_CONFIG_PATH,
	prelude::{Result, eyre},
};

pub(crate) fn short_sha(value: &str) -> String {
	value.chars().take(7).collect()
}

pub(crate) fn slugify(value: &str) -> String {
	let mut slug = String::new();
	let mut previous_was_separator = false;

	for character in value.chars().flat_map(char::to_lowercase) {
		if character.is_ascii_lowercase() || character.is_ascii_digit() {
			slug.push(character);

			previous_was_separator = false;
		} else if !previous_was_separator && !slug.is_empty() {
			slug.push('-');

			previous_was_separator = true;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { "signal".into() } else { slug }
}

pub(crate) fn repo_root() -> Result<PathBuf> {
	let mut candidate = env::current_dir()?;

	loop {
		if candidate.join(DEFAULT_CONFIG_PATH).is_file()
			&& candidate.join("apps/radar/src/lib.rs").is_file()
		{
			return Ok(candidate);
		}
		if !candidate.pop() {
			return Err(eyre::eyre!(
				"Unable to find Decodex repository root from current directory"
			));
		}
	}
}

pub(crate) fn resolve_against(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn path_arg(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).display().to_string()
}

pub(crate) fn pretty_json(payload: &Value) -> Result<String> {
	serde_json::to_string_pretty(payload).map_err(Into::into)
}

pub(crate) fn body_excerpt(body: &str) -> String {
	let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");

	if compact.chars().count() > 500 {
		format!("{}...", compact.chars().take(500).collect::<String>())
	} else {
		compact
	}
}

pub(crate) fn required_value_string(payload: &Value, field: &str) -> Result<String> {
	payload
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.ok_or_else(|| eyre::eyre!("{field} must be a non-empty string"))
}

pub(crate) fn optional_value_string(payload: &Value, field: &str) -> Option<String> {
	payload.get(field).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned)
}

pub(crate) fn required_value_u64(payload: &Value, field: &str) -> Result<u64> {
	payload
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{field} must be a positive integer"))
}

pub(crate) fn required_value_i64(payload: &Value, field: &str) -> Result<i64> {
	payload
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{field} must be an integer"))
}

pub(crate) fn truncate_patch_excerpt(value: &str) -> String {
	let compact = value.trim();

	if compact.chars().count() > 900 {
		format!("{}...", compact.chars().take(900).collect::<String>())
	} else {
		compact.to_owned()
	}
}

pub(crate) fn string_array(value: Option<&Value>) -> Vec<String> {
	value
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|item| item.as_str().map(str::to_owned))
		.collect()
}

pub(crate) fn string_array_from_value(value: &Value) -> Vec<String> {
	string_array(Some(value))
}

pub(crate) fn extract_commit_sha_from_url(url: &str) -> Option<String> {
	let sha = url.rsplit_once("/commit/")?.1;

	(sha.len() >= 7 && sha.len() <= 40 && sha.chars().all(|ch| ch.is_ascii_hexdigit()))
		.then(|| sha.to_owned())
}

pub(crate) fn extract_pr_number_from_url(url: &str) -> Option<u64> {
	let number = url.rsplit_once("/pull/")?.1;

	(!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit()))
		.then(|| number.parse::<u64>().ok())
		.flatten()
}

pub(crate) fn percent_encode(value: &str) -> String {
	let mut encoded = String::new();

	for byte in value.bytes() {
		if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
			encoded.push(char::from(byte));
		} else {
			encoded.push_str(&format!("%{byte:02X}"));
		}
	}

	encoded
}
