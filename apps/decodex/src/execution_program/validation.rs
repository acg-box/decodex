//! Shared validation helpers for execution-program payloads.

use crate::{
	execution_program::model::{EXECUTION_PROGRAM_RECORD_VERSION, EXECUTION_PROGRAM_SCHEMA},
	prelude::{Result, eyre},
};

pub(super) fn execution_program_schema() -> String {
	EXECUTION_PROGRAM_SCHEMA.to_owned()
}

pub(super) fn execution_program_record_version() -> u16 {
	EXECUTION_PROGRAM_RECORD_VERSION
}

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_optional(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn non_empty_optional(value: &str) -> Option<&str> {
	if value.is_empty() { None } else { Some(value) }
}

pub(super) fn is_false(value: &bool) -> bool {
	!*value
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}
