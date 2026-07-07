use crate::{
	commit_message::{
		model::{COMMIT_MESSAGE_SCHEMA, CommitMessage},
		normalize, parse,
	},
	prelude::{Result, eyre},
};

pub(crate) fn build_commit_message(
	summary: &str,
	authority: &str,
	related: &[String],
	breaking: bool,
) -> Result<String> {
	if !related.is_empty() {
		eyre::bail!("`decodex/commit/2` is commit-local and does not accept related issues");
	}
	let summary = normalize::normalize_single_line_field("summary", summary)?;
	let authority = normalize::normalize_commit_authority("authority", authority)?;
	let impact = commit_impact(breaking);

	serde_json::to_string(&CommitMessage {
		schema: COMMIT_MESSAGE_SCHEMA,
		change: summary.as_str(),
		authority: authority.as_str(),
		impact,
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
	let landed_summary = landing_summary(&record.change);
	let authority =
		normalize::normalize_commit_authority("expected_authority", expected_authority)?;
	let breaking = record.impact == "breaking";

	build_commit_message(&landed_summary, &authority, &[], breaking)
}

pub(crate) fn validate_commit_message_subject(message: &str) -> Result<()> {
	parse::parse_commit_message_record(message, None)?;

	Ok(())
}

fn landing_summary(summary: &str) -> String {
	let summary = summary.strip_prefix("Land ").unwrap_or(summary);

	format!("Land {summary}")
}

fn commit_impact(breaking: bool) -> &'static str {
	if breaking { "breaking" } else { "compatible" }
}
