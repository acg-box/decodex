use std::path::{Component, Path};

pub(in crate::tracker::records::validation) fn validate_repo_relative_path(
	path: &str,
	field_name: &str,
) -> Result<(), String> {
	if path.is_empty() {
		return Err(format!("`{field_name}` must not be empty."));
	}
	if path.starts_with('/') || path.starts_with("~/") || has_drive_root_prefix(path) {
		return Err(format!("`{field_name}` must be repository-relative, not `{path}`."));
	}
	if Path::new(path).components().any(|component| matches!(component, Component::ParentDir)) {
		return Err(format!("`{field_name}` must stay within the repository, not `{path}`."));
	}

	Ok(())
}

fn has_drive_root_prefix(path: &str) -> bool {
	let bytes = path.as_bytes();

	bytes.len() >= 3
		&& bytes[0].is_ascii_alphabetic()
		&& bytes[1] == b':'
		&& matches!(bytes[2], b'\\' | b'/')
}
