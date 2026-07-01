//! Shared Objective Contract validation helpers.

use crate::prelude::{Result, eyre};

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_nonempty_list(name: &str, values: &[String]) -> Result<()> {
	if values.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	validate_string_list(name, values)
}
