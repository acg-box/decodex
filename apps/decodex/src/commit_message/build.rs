use crate::{
	commit_message::{
		model::{COMMIT_MESSAGE_SCHEMA, CommitMessage},
		normalize, parse,
	},
	prelude::Result,
};

pub(crate) fn build_commit_message(
	summary: &str,
	authority: &str,
	related: &[String],
	breaking: bool,
) -> Result<String> {
	let summary = normalize::normalize_single_line_field("summary", summary)?;
	let authority = normalize::normalize_commit_authority("authority", authority)?;
	let related = related
		.iter()
		.map(|value| normalize::normalize_issue_identifier("related", value))
		.collect::<Result<Vec<_>>>()?;

	serde_json::to_string(&CommitMessage {
		schema: COMMIT_MESSAGE_SCHEMA,
		summary: summary.as_str(),
		authority: authority.as_str(),
		related,
		breaking,
	})
	.map_err(Into::into)
}

pub(crate) fn build_landing_commit_message(
	summary: &str,
	authority: &str,
	related: &[String],
	breaking: bool,
) -> Result<String> {
	let summary = normalize::normalize_single_line_field("summary", summary)?;
	let landed_summary = landing_summary(&summary);

	build_commit_message(&landed_summary, authority, related, breaking)
}

pub(crate) fn build_landed_merge_commit_message(
	head_message: &str,
	expected_authority: &str,
) -> Result<String> {
	let record = parse::parse_commit_message_record(head_message, Some(expected_authority))?;
	let landed_summary = landing_summary(&record.summary);
	let authority =
		normalize::normalize_commit_authority("expected_authority", expected_authority)?;

	build_commit_message(&landed_summary, &authority, &record.related, record.breaking)
}

pub(crate) fn validate_commit_message_subject(message: &str) -> Result<()> {
	parse::parse_commit_message_record(message, None)?;

	Ok(())
}

fn landing_summary(summary: &str) -> String {
	let summary = summary.strip_prefix("Land ").unwrap_or(summary);

	format!("Land {summary}")
}
