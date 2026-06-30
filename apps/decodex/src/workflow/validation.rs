use std::path::{Component, Path};

use crate::prelude::eyre;

pub(super) const FRONTMATTER_DELIMITER: &str = "+++";

pub(super) fn validate_string_entries(
	field_name: &str,
	values: &[String],
) -> crate::prelude::Result<()> {
	for value in values {
		let trimmed = value.trim();

		if trimmed.is_empty() {
			eyre::bail!("`{field_name}` entries must not be empty.");
		}
		if trimmed != value {
			eyre::bail!("`{field_name}` entries must not include surrounding whitespace.");
		}
	}

	Ok(())
}

pub(super) fn validate_repo_relative_paths(
	field_name: &str,
	values: &[String],
) -> crate::prelude::Result<()> {
	validate_string_entries(field_name, values)?;

	for value in values {
		let path = Path::new(value);

		if path.is_absolute() {
			eyre::bail!("`{field_name}` entries must be repository-relative paths.");
		}
		if !path.components().all(|component| matches!(component, Component::Normal(_))) {
			eyre::bail!(
				"`{field_name}` entries must not contain `.`, `..`, root, or prefix components."
			);
		}
	}

	Ok(())
}

pub(super) fn split_frontmatter(input: &str) -> crate::prelude::Result<(String, String)> {
	let input = input.trim_start_matches(['\u{feff}', '\n', '\r']);
	let mut lines = input.lines();

	if lines.next() != Some(FRONTMATTER_DELIMITER) {
		eyre::bail!("WORKFLOW.md must begin with TOML frontmatter delimited by `+++`.");
	}

	let mut frontmatter_lines = Vec::new();
	let mut body_lines = Vec::new();
	let mut found_end = false;

	for line in lines {
		if !found_end && line == FRONTMATTER_DELIMITER {
			found_end = true;

			continue;
		}
		if found_end {
			body_lines.push(line);
		} else {
			frontmatter_lines.push(line);
		}
	}

	if !found_end {
		eyre::bail!("WORKFLOW.md frontmatter is missing the closing `+++` delimiter.");
	}

	let body = body_lines.join("\n").trim().to_string();

	Ok((frontmatter_lines.join("\n"), body))
}

pub(super) fn validate_trimmed_non_empty(
	field_name: &str,
	value: &str,
) -> crate::prelude::Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if value != value.trim() {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}

	Ok(())
}

pub(super) fn validate_non_empty_string_list(
	field_name: &str,
	values: &[String],
) -> crate::prelude::Result<()> {
	if values.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}

	for value in values {
		validate_trimmed_non_empty(&format!("{field_name} entries"), value)?;
	}

	Ok(())
}
