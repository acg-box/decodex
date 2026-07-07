use std::{fs, io::ErrorKind, path::PathBuf};

use crate::{
	mcp::{McpError, resources::types::ResourceContent},
	prelude::Result,
};

pub(super) fn read_file_resource(
	uri: &str,
	path: PathBuf,
	mime_type: &str,
) -> Result<ResourceContent, McpError> {
	let text = fs::read_to_string(path).map_err(|error| match error.kind() {
		ErrorKind::NotFound => McpError::resource_not_found(),
		_ => McpError::internal(error),
	})?;

	Ok(ResourceContent { uri: uri.to_owned(), mime_type: mime_type.to_owned(), text })
}

pub(super) fn docs_lane_allowed(lane: &str) -> bool {
	matches!(lane, "spec" | "runbook" | "reference" | "decisions")
}

pub(super) fn safe_resource_stem(value: &str) -> bool {
	!value.is_empty()
		&& value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
