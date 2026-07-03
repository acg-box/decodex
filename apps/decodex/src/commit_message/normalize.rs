use crate::{
	commit_message::model::MANUAL_AUTHORITY,
	prelude::{Result, eyre},
};

pub(crate) fn looks_like_issue_identifier(value: &str) -> bool {
	let Some((prefix, number)) = value.rsplit_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& !number.is_empty()
		&& prefix.chars().all(|character| character.is_ascii_alphanumeric())
		&& number.chars().all(|character| character.is_ascii_digit())
}

pub(crate) fn normalize_single_line_field(field_name: &str, value: &str) -> Result<String> {
	let trimmed = value.trim();

	if trimmed.is_empty() {
		eyre::bail!("`{field_name}` must not be empty.");
	}
	if trimmed != value {
		eyre::bail!("`{field_name}` must not include surrounding whitespace.");
	}
	if trimmed.contains('\n') || trimmed.contains('\r') {
		eyre::bail!("`{field_name}` must stay on one line.");
	}

	Ok(trimmed.to_owned())
}

pub(crate) fn normalize_issue_identifier(field_name: &str, value: &str) -> Result<String> {
	let normalized = normalize_single_line_field(field_name, value)?;

	if !looks_like_issue_identifier(&normalized) {
		eyre::bail!("`{field_name}` must look like an issue identifier such as `XY-123`.");
	}

	Ok(normalized)
}

pub(crate) fn normalize_commit_authority(field_name: &str, value: &str) -> Result<String> {
	let normalized = normalize_single_line_field(field_name, value)?;

	if normalized == MANUAL_AUTHORITY || looks_like_issue_identifier(&normalized) {
		return Ok(normalized);
	}

	eyre::bail!(
		"`{field_name}` must look like an issue identifier such as `XY-123` or be exactly `{MANUAL_AUTHORITY}`."
	);
}
