use std::path::{Component, Path};

use crate::tracker::public_text;

pub(crate) fn validate_public_comment_body(body: &str) -> Result<(), String> {
	public_text::validate_public_text_field("body", body)?;

	for line in body.lines() {
		let Some((field_name, value)) = self::extract_structured_field(line) else {
			continue;
		};

		if field_name == "worktree_path" {
			self::validate_repo_relative_path(value, field_name)?;

			continue;
		}
		if field_name.ends_with("_path") {
			return Err(format!(
				"Unsupported structured field `{field_name}` in public issue comments."
			));
		}
	}

	Ok(())
}

fn extract_structured_field(line: &str) -> Option<(&str, &str)> {
	let trimmed = line.trim();
	let trimmed = trimmed.strip_prefix("- ").unwrap_or(trimmed);
	let (key, value) = trimmed.split_once(':')?;

	Some((key.trim(), value.trim().trim_matches('`')))
}

fn validate_repo_relative_path(path: &str, field_name: &str) -> Result<(), String> {
	if path.is_empty() {
		return Err(format!("`{field_name}` must not be empty."));
	}
	if path.starts_with('/') || path.starts_with("~/") || self::has_drive_root_prefix(path) {
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
