use crate::prelude::{Result, eyre};

pub(in crate::research_design) fn normalize_required_text(
	name: &str,
	value: impl Into<String>,
) -> Result<String> {
	let value = value.into();
	let value = value.trim();

	if value.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(value.to_owned())
}

pub(in crate::research_design) fn normalize_optional_text(
	name: &str,
	value: Option<String>,
) -> Result<Option<String>> {
	value.map(|value| normalize_required_text(name, value)).transpose()
}

pub(in crate::research_design) fn normalize_text_list(
	name: &str,
	values: Vec<String>,
) -> Result<Vec<String>> {
	values.into_iter().map(|value| normalize_required_text(name, value)).collect()
}
