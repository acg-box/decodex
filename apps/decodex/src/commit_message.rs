use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

pub(crate) const COMMIT_MESSAGE_SCHEMA: &str = "decodex/commit/1";
pub(crate) const MANUAL_AUTHORITY: &str = "manual";

#[derive(Serialize)]
struct CommitMessage<'a> {
	schema: &'static str,
	summary: &'a str,
	authority: &'a str,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	related: Vec<String>,
	#[serde(skip_serializing_if = "is_false")]
	breaking: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitMessageRecord {
	schema: String,
	summary: String,
	authority: String,
	#[serde(default)]
	related: Vec<String>,
	#[serde(default)]
	breaking: bool,
}

pub(crate) fn build_commit_message(
	summary: &str,
	authority: &str,
	related: &[String],
	breaking: bool,
) -> Result<String> {
	let summary = normalize_single_line_field("summary", summary)?;
	let authority = normalize_commit_authority("authority", authority)?;
	let related = related
		.iter()
		.map(|value| normalize_issue_identifier("related", value))
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
	let summary = normalize_single_line_field("summary", summary)?;
	let landed_summary = landing_summary(&summary);

	build_commit_message(&landed_summary, authority, related, breaking)
}

pub(crate) fn build_landed_merge_commit_message(
	head_message: &str,
	expected_authority: &str,
) -> Result<String> {
	let record = parse_commit_message_record(head_message, Some(expected_authority))?;
	let landed_summary = landing_summary(&record.summary);
	let authority = normalize_commit_authority("expected_authority", expected_authority)?;

	build_commit_message(&landed_summary, &authority, &record.related, record.breaking)
}

pub(crate) fn validate_commit_message_subject(message: &str) -> Result<()> {
	parse_commit_message_record(message, None)?;

	Ok(())
}

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

fn landing_summary(summary: &str) -> String {
	let summary = summary.strip_prefix("Land ").unwrap_or(summary);

	format!("Land {summary}")
}

fn parse_commit_message_record(
	message: &str,
	expected_authority: Option<&str>,
) -> Result<CommitMessageRecord> {
	let message = normalize_single_line_field("commit_message", message)?;
	let mut record: CommitMessageRecord = serde_json::from_str(&message)?;

	if record.schema != COMMIT_MESSAGE_SCHEMA {
		eyre::bail!(
			"`commit_message.schema` must be `{COMMIT_MESSAGE_SCHEMA}`, not `{}`.",
			record.schema
		);
	}

	record.summary = normalize_single_line_field("summary", &record.summary)?;

	let authority = normalize_commit_authority("authority", &record.authority)?;

	if let Some(expected_authority) = expected_authority {
		let expected_authority =
			normalize_commit_authority("expected_authority", expected_authority)?;

		if !authority.eq_ignore_ascii_case(&expected_authority) {
			eyre::bail!(
				"`commit_message.authority` `{authority}` does not match expected authority `{expected_authority}`."
			);
		}
	}

	for related in &mut record.related {
		*related = normalize_issue_identifier("related", related)?;
	}

	Ok(record)
}

fn is_false(value: &bool) -> bool {
	!value
}

#[cfg(test)]
mod tests {
	use crate::commit_message::{self};

	#[test]
	fn build_commit_message_omits_empty_related_and_false_breaking() {
		let message =
			commit_message::build_commit_message("tighten workflow defaults", "XY-225", &[], false)
				.expect("commit message should build");

		assert_eq!(
			message,
			r#"{"schema":"decodex/commit/1","summary":"tighten workflow defaults","authority":"XY-225"}"#
		);
	}

	#[test]
	fn build_commit_message_includes_optional_fields() {
		let message = commit_message::build_commit_message(
			"tighten workflow defaults",
			"XY-225",
			&[String::from("XY-12"), String::from("XY-99")],
			true,
		)
		.expect("commit message should build");

		assert_eq!(
			message,
			r#"{"schema":"decodex/commit/1","summary":"tighten workflow defaults","authority":"XY-225","related":["XY-12","XY-99"],"breaking":true}"#
		);
	}

	#[test]
	fn build_landing_commit_message_normalizes_land_prefix() {
		for (summary, related, breaking, expected) in [
			(
				"tighten workflow defaults",
				vec![String::from("XY-12")],
				true,
				r#"{"schema":"decodex/commit/1","summary":"Land tighten workflow defaults","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
			),
			(
				"Land tighten workflow defaults",
				Vec::new(),
				false,
				r#"{"schema":"decodex/commit/1","summary":"Land tighten workflow defaults","authority":"XY-225"}"#,
			),
		] {
			let message =
				commit_message::build_landing_commit_message(summary, "XY-225", &related, breaking)
					.expect("landing commit message should build");

			assert_eq!(message, expected);
		}
	}

	#[test]
	fn build_commit_message_rejects_multiline_summary() {
		let error = commit_message::build_commit_message("one\ntwo", "XY-225", &[], false)
			.expect_err("multiline summary should fail");

		assert!(error.to_string().contains("must stay on one line"));
	}

	#[test]
	fn build_commit_message_accepts_manual_authority() {
		let message = commit_message::build_commit_message(
			"ship hotfix outside tracker",
			"manual",
			&[],
			false,
		)
		.expect("manual authority should build");

		assert_eq!(
			message,
			r#"{"schema":"decodex/commit/1","summary":"ship hotfix outside tracker","authority":"manual"}"#
		);
	}

	#[test]
	fn looks_like_issue_identifier_requires_suffix_number() {
		assert!(commit_message::looks_like_issue_identifier("XY-225"));
		assert!(commit_message::looks_like_issue_identifier("A1-9"));
		assert!(!commit_message::looks_like_issue_identifier("XY"));
		assert!(!commit_message::looks_like_issue_identifier("XY-"));
		assert!(!commit_message::looks_like_issue_identifier("-123"));
		assert!(!commit_message::looks_like_issue_identifier("XY-12A"));
	}

	#[test]
	fn build_landed_merge_commit_message_normalizes_land_prefix() {
		for (head_message, expected) in [
			(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
				r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
			),
			(
				r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225"}"#,
				r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225"}"#,
			),
		] {
			let landed_message =
				commit_message::build_landed_merge_commit_message(head_message, "XY-225")
					.expect("landed merge message should build");

			assert_eq!(landed_message, expected);
		}
	}

	#[test]
	fn build_landed_merge_commit_message_rejects_invalid_head_subjects() {
		for (head_message, authority, expected) in [
			(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"XY-225"}"#,
				"XY-226",
				"does not match expected authority",
			),
			("ship fix", "XY-225", "expected value"),
		] {
			let error = commit_message::build_landed_merge_commit_message(head_message, authority)
				.expect_err("invalid landed head subject should fail");

			assert!(error.to_string().contains(expected));
		}
	}

	#[test]
	fn validate_commit_message_subject_accepts_schema_record_without_expected_authority() {
		commit_message::validate_commit_message_subject(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#,
		)
		.expect("manual schema subject should validate");
	}

	#[test]
	fn validate_commit_message_subject_rejects_non_schema_records() {
		for (message, expected) in [
			("ship fix", "expected value"),
			(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual","extra":true}"#,
				"unknown field",
			),
			(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual","related":["not-an-issue"]}"#,
				"issue identifier",
			),
			(
				r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"not-an-issue"}"#,
				"authority",
			),
		] {
			let error = commit_message::validate_commit_message_subject(message)
				.expect_err("invalid schema subject should fail");

			assert!(
				error.to_string().contains(expected),
				"`{message}` failed with unexpected error: {error}"
			);
		}
	}
}
