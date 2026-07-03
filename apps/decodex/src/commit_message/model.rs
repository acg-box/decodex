use serde::{Deserialize, Serialize};

pub(crate) const COMMIT_MESSAGE_SCHEMA: &str = "decodex/commit/1";
pub(crate) const MANUAL_AUTHORITY: &str = "manual";

#[derive(Serialize)]
pub(super) struct CommitMessage<'a> {
	pub(super) schema: &'static str,
	pub(super) summary: &'a str,
	pub(super) authority: &'a str,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	pub(super) related: Vec<String>,
	#[serde(skip_serializing_if = "is_false")]
	pub(super) breaking: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommitMessageRecord {
	pub(super) schema: String,
	pub(super) summary: String,
	pub(super) authority: String,
	#[serde(default)]
	pub(super) related: Vec<String>,
	#[serde(default)]
	pub(super) breaking: bool,
}

fn is_false(value: &bool) -> bool {
	!value
}
