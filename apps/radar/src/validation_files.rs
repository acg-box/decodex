//! Artifact validation file traversal and JSON I/O helpers.

mod fields;
mod files;
mod freshness;
mod json_io;
mod report;

pub(crate) use self::{
	fields::{
		first_line, is_truthy_json_value, non_empty_array, object_value, optional_string,
		require_member, required_string, string_field, utc_now_iso,
	},
	files::{collect_json_files, validation_paths},
	freshness::{is_default_source_snapshot, validate_source_freshness},
	json_io::{load_json, write_json},
	report::queue_report,
};
