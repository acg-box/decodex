mod constants;
mod detection;
mod structured;

pub(crate) use structured::validate_public_comment_body;

pub(crate) fn validate_public_text_field(field_name: &str, value: &str) -> Result<(), String> {
	if let Some(reason) = detection::public_text_violation(value) {
		return Err(format!("`{field_name}` must be public/team-visible text; {reason}."));
	}

	Ok(())
}

pub(crate) fn validate_public_text_items(
	field_name: &str,
	values: &[String],
) -> Result<(), String> {
	for value in values {
		validate_public_text_field(field_name, value)?;
	}

	Ok(())
}

#[cfg(test)] mod tests;
