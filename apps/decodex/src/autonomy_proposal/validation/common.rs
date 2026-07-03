use std::collections::BTreeSet;

use crate::prelude::{Result, eyre};

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_optional_required(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_sorted_unique(name: &str, values: &[String]) -> Result<()> {
	validate_string_list(name, values)?;

	let mut seen = BTreeSet::new();
	let mut previous = None;

	for value in values {
		if previous.is_some_and(|previous| previous > value.as_str()) {
			eyre::bail!("{name} must be sorted.");
		}
		if !seen.insert(value.as_str()) {
			eyre::bail!("{name} must not contain duplicates.");
		}

		previous = Some(value.as_str());
	}

	Ok(())
}

pub(super) fn unique_sorted_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
	values
		.into_iter()
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}
