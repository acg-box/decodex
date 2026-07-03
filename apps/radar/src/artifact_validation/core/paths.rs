use std::path::Path;

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	artifact_validation::{
		RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF, UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF,
		UPSTREAM_REVIEW_SCHEMA, constants::RADAR_ARCHIVE_MANIFEST_SCHEMA, support,
	},
	prelude::eyre::Report,
};

pub(super) fn is_analysis_draft_path(path: &Path) -> bool {
	let normalized = normalized_path(path);

	normalized.ends_with(".analysis.json")
		&& (normalized.contains("/generated/analysis/")
			|| normalized.starts_with("generated/analysis/"))
}

pub(super) fn is_historical_archive_manifest_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	support::string_field(entry, "schema") == Some(RADAR_ARCHIVE_MANIFEST_SCHEMA)
		&& normalized.contains("/cache/archive/index/")
		&& timestamp_field_before(entry, "created_at", RADAR_ARCHIVE_HISTORICAL_RETENTION_CUTOFF)
}

pub(super) fn is_historical_upstream_review_path(path: &Path, payload: &Value) -> bool {
	let Some(entry) = payload.as_object() else {
		return false;
	};
	let normalized = normalized_path(path);

	support::string_field(entry, "schema") == Some(UPSTREAM_REVIEW_SCHEMA)
		&& normalized.contains("/cache/github/reviews/")
		&& timestamp_field_before(entry, "reviewed_at", UPSTREAM_REVIEW_LINEAR_FOLLOWUP_CUTOFF)
}

pub(super) fn analysis_draft_error_lines(error: Report) -> Vec<String> {
	error
		.to_string()
		.lines()
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| line.trim_start_matches("- ").to_owned())
		.collect()
}

fn timestamp_field_before(entry: &Map<String, Value>, field: &str, cutoff: &str) -> bool {
	let Some(value) = entry.get(field).and_then(Value::as_str) else {
		return false;
	};
	let Ok(value) = OffsetDateTime::parse(value, &Rfc3339) else {
		return false;
	};
	let Ok(cutoff) = OffsetDateTime::parse(cutoff, &Rfc3339) else {
		return false;
	};

	value < cutoff
}

fn normalized_path(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}
