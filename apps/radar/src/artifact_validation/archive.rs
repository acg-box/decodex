//! Radar archive manifest validation.

use serde_json::{Map, Value};

use super::{
	model::ArtifactValidationOptions,
	support::{
		is_https_string, is_non_empty_string, is_sha256_hex, matches_one_of, non_empty_array,
		validate_rfc3339_field,
	},
};

pub(super) fn validate_radar_archive_manifest(
	entry: &Map<String, Value>,
	options: ArtifactValidationOptions,
	errors: &mut Vec<String>,
) {
	for field in ["archive_id", "created_at", "source_commit", "release_tag", "release_url"] {
		if !is_non_empty_string(entry.get(field)) {
			errors.push(format!("{field} must be a non-empty string"));
		}
	}

	validate_rfc3339_field(entry, "created_at", errors);

	if entry.get("retention_days").and_then(Value::as_u64) != Some(21)
		&& !options.allow_historical_archive_retention
	{
		errors.push("retention_days must be 21".into());
	}
	if !is_https_string(entry.get("release_url")) {
		errors.push("release_url must be an https URL".into());
	}

	validate_archive_asset(entry.get("archive_asset"), "archive_asset", true, errors);
	validate_archive_asset(entry.get("checksum_asset"), "checksum_asset", false, errors);
	validate_archive_files(entry.get("files"), errors);
}

pub(super) fn validate_archive_asset(
	value: Option<&Value>,
	label: &str,
	require_size: bool,
	errors: &mut Vec<String>,
) {
	let Some(asset) = value.and_then(Value::as_object) else {
		errors.push(format!("{label} must be an object"));

		return;
	};

	if !is_non_empty_string(asset.get("name")) {
		errors.push(format!("{label}.name must be a non-empty string"));
	}
	if !asset.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_hex) {
		errors.push(format!("{label}.sha256 must be a SHA-256 hex digest"));
	}
	if require_size && asset.get("size_bytes").and_then(Value::as_u64).is_none_or(|size| size == 0)
	{
		errors.push(format!("{label}.size_bytes must be a positive integer"));
	}
}

pub(super) fn validate_archive_files(value: Option<&Value>, errors: &mut Vec<String>) {
	let Some(files) = non_empty_array(value) else {
		errors.push("files must be a non-empty list".into());

		return;
	};

	for (index, file) in files.iter().enumerate() {
		let Some(file) = file.as_object() else {
			errors.push(format!("files[{index}] must be an object"));

			continue;
		};

		for field in ["path", "kind"] {
			if !is_non_empty_string(file.get(field)) {
				errors.push(format!("files[{index}].{field} must be a non-empty string"));
			}
		}

		if !matches_one_of(
			file.get("kind"),
			&["analysis", "bundle", "ledger_export", "other", "source_cache"],
		) {
			errors.push(format!(
				"files[{index}].kind must be one of ['analysis', 'bundle', 'ledger_export', 'other', 'source_cache']"
			));
		}
		if !file.get("sha256").and_then(Value::as_str).is_some_and(is_sha256_hex) {
			errors.push(format!("files[{index}].sha256 must be a SHA-256 hex digest"));
		}
		if file.get("size_bytes").and_then(Value::as_u64).is_none_or(|size| size == 0) {
			errors.push(format!("files[{index}].size_bytes must be a positive integer"));
		}
	}
}
