use std::{collections::BTreeMap, fs, path::Path};

use serde_json::{self, Value};

use crate::{
	agent::app_server::schema_probe::constants::APP_SERVER_SCHEMA_PROSE_KEYS, prelude::Result,
};

pub(in crate::agent::app_server::schema_probe) fn collect_schema_markers(
	path: &Path,
	marker_presence: &mut BTreeMap<&'static str, bool>,
) -> Result<usize> {
	let mut json_file_count = 0;

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();

		if path.is_dir() {
			json_file_count += collect_schema_markers(&path, marker_presence)?;
		} else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
			let schema = fs::read_to_string(&path)?;
			let value: Value = serde_json::from_str(&schema)?;

			json_file_count += 1;

			record_schema_markers_from_value(&value, marker_presence);
		}
	}

	Ok(json_file_count)
}

fn record_schema_markers_from_value(
	value: &Value,
	marker_presence: &mut BTreeMap<&'static str, bool>,
) {
	match value {
		Value::Object(object) =>
			for (key, value) in object {
				if schema_prose_key(key) {
					continue;
				}

				record_schema_marker_from_text(key, marker_presence);
				record_schema_markers_from_value(value, marker_presence);
			},
		Value::Array(values) =>
			for value in values {
				record_schema_markers_from_value(value, marker_presence);
			},
		Value::String(value) => record_schema_marker_from_text(value, marker_presence),
		Value::Null | Value::Bool(_) | Value::Number(_) => {},
	}
}

fn schema_prose_key(key: &str) -> bool {
	APP_SERVER_SCHEMA_PROSE_KEYS.contains(&key)
}

fn record_schema_marker_from_text(value: &str, marker_presence: &mut BTreeMap<&'static str, bool>) {
	for (marker, present) in marker_presence {
		if value.contains(*marker) {
			*present = true;
		}
	}
}
