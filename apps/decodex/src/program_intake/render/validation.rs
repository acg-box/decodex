use crate::{
	prelude::{Result, eyre},
	tracker::public_text,
};

pub(crate) fn validate_generated_issue_text(
	title: &str,
	description: &str,
	private_identifiers: &[&str],
) -> Result<()> {
	public_text::validate_public_text_field("generated issue title", title)
		.map_err(|error| eyre::eyre!(error))?;

	ensure_no_generated_issue_private_identifier(
		"generated issue title",
		title,
		private_identifiers,
	)?;
	validate_public_issue_description(description)?;

	ensure_no_generated_issue_private_identifier(
		"generated issue description",
		description,
		private_identifiers,
	)
}

pub(in crate::program_intake::render) fn validate_public_issue_description(
	description: &str,
) -> Result<()> {
	public_text::validate_public_text_field("generated issue description", description)
		.map_err(|error| eyre::eyre!(error))
}

fn ensure_no_generated_issue_private_identifier(
	field: &str,
	text: &str,
	private_identifiers: &[&str],
) -> Result<()> {
	for identifier in private_identifiers {
		if !identifier.is_empty() && text.contains(identifier) {
			eyre::bail!("{field} contains a private Program Intake identifier.");
		}
	}

	Ok(())
}
