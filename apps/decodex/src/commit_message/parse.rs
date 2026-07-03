use crate::{
	commit_message::{
		model::{COMMIT_MESSAGE_SCHEMA, CommitMessageRecord},
		normalize,
	},
	prelude::{Result, eyre},
};

pub(super) fn parse_commit_message_record(
	message: &str,
	expected_authority: Option<&str>,
) -> Result<CommitMessageRecord> {
	let message = normalize::normalize_single_line_field("commit_message", message)?;
	let mut record: CommitMessageRecord = serde_json::from_str(&message)?;

	if record.schema != COMMIT_MESSAGE_SCHEMA {
		eyre::bail!(
			"`commit_message.schema` must be `{COMMIT_MESSAGE_SCHEMA}`, not `{}`.",
			record.schema
		);
	}

	record.summary = normalize::normalize_single_line_field("summary", &record.summary)?;

	let authority = normalize::normalize_commit_authority("authority", &record.authority)?;

	if let Some(expected_authority) = expected_authority {
		let expected_authority =
			normalize::normalize_commit_authority("expected_authority", expected_authority)?;

		if !authority.eq_ignore_ascii_case(&expected_authority) {
			eyre::bail!(
				"`commit_message.authority` `{authority}` does not match expected authority `{expected_authority}`."
			);
		}
	}

	for related in &mut record.related {
		*related = normalize::normalize_issue_identifier("related", related)?;
	}

	Ok(record)
}
