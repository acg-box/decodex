//! Artifact validation file traversal and JSON I/O helpers.

use std::{
	fs::{self, OpenOptions},
	io::Write as _,
	path::{Path, PathBuf},
	process,
};

use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{DEFAULT_VALIDATION_PATHS, RadarRefreshQueueReport, prelude::eyre};

pub(crate) fn queue_report(
	queue: &Value,
	changed: bool,
	ledger_enabled: bool,
	root: &Path,
	queue_out: &Path,
) -> RadarRefreshQueueReport {
	let counts = queue.get("counts").and_then(Value::as_object);

	RadarRefreshQueueReport {
		changed,
		recent_commits_scanned: count_field(counts, "recent_commits_scanned"),
		published_subjects_seen: count_field(counts, "published_subjects_seen"),
		subjects_queued: count_field(counts, "subjects_queued"),
		ledger_enabled,
		queue_out: crate::absolute_repo_path(root, queue_out),
	}
}

pub(crate) fn count_field(counts: Option<&Map<String, Value>>, field: &str) -> usize {
	counts
		.and_then(|counts| counts.get(field))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())
		.unwrap_or_default()
}

pub(crate) fn validation_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
	if paths.is_empty() {
		DEFAULT_VALIDATION_PATHS.iter().map(PathBuf::from).collect()
	} else {
		paths.to_vec()
	}
}

pub(crate) fn collect_json_files(paths: &[PathBuf]) -> crate::prelude::Result<Vec<PathBuf>> {
	let mut files = Vec::new();

	for path in paths {
		collect_json_path(path, &mut files)?;
	}

	files.sort();

	Ok(files)
}

pub(crate) fn collect_json_path(
	path: &Path,
	files: &mut Vec<PathBuf>,
) -> crate::prelude::Result<()> {
	if path.is_dir() {
		let mut children = fs::read_dir(path)?
			.map(|entry| entry.map(|entry| entry.path()))
			.collect::<std::result::Result<Vec<_>, _>>()?;

		children.sort();

		for child in children {
			collect_json_path(&child, files)?;
		}
	} else if path.is_file() {
		if path.extension().is_some_and(|extension| extension == "json") {
			files.push(path.to_path_buf());
		}
	} else {
		return Err(eyre::eyre!("Radar validation path does not exist: {}", path.display()));
	}

	Ok(())
}

pub(crate) fn load_json(path: &Path) -> crate::prelude::Result<Value> {
	let raw = fs::read_to_string(path)?;

	serde_json::from_str(&raw)
		.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
}

pub(crate) fn write_json(path: &Path, payload: &Value) -> crate::prelude::Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("JSON output path must end in a valid file name"))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let write_result = (|| -> crate::prelude::Result<()> {
		let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

		file.write_all(output.as_bytes())?;
		file.sync_all()?;

		fs::rename(&temp_path, path)?;

		Ok(())
	})();

	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	write_result?;

	Ok(())
}

pub(crate) fn require_member(
	value: &str,
	allowed: &[&str],
	label: &str,
) -> crate::prelude::Result<()> {
	if allowed.contains(&value) {
		Ok(())
	} else {
		eyre::bail!("{label} must be one of {}", choices(allowed))
	}
}

pub(crate) fn choices(values: &[&str]) -> String {
	let quoted = values.iter().map(|value| format!("'{value}'")).collect::<Vec<_>>().join(", ");

	format!("[{quoted}]")
}

pub(crate) fn utc_now_iso() -> crate::prelude::Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

pub(crate) fn object_value<'a>(
	value: &'a Value,
	label: &str,
) -> crate::prelude::Result<&'a Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

pub(crate) fn string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(crate) fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a str> {
	string_field(object, field)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty string"))
}

pub(crate) fn optional_string<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
	object.get(field).and_then(Value::as_str)
}

pub(crate) fn is_truthy_json_value(value: Option<&Value>) -> bool {
	match value {
		Some(Value::Null) | None => false,
		Some(Value::String(value)) => !value.is_empty(),
		Some(_) => true,
	}
}

pub(crate) fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
	value.and_then(Value::as_array).filter(|values| !values.is_empty())
}

pub(crate) fn first_line(value: &str) -> String {
	value.trim().lines().next().unwrap_or("").into()
}
