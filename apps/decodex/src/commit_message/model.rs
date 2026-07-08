use serde::{Deserialize, Serialize};

pub(crate) const COMMIT_MESSAGE_SCHEMA: &str = "decodex/commit/2";
pub(crate) const BASELINE_AUTHORITY: &str = "baseline";
pub(crate) const MANUAL_AUTHORITY: &str = "manual";

#[derive(Serialize)]
pub(super) struct CommitMessage<'a> {
	pub(super) schema: &'static str,
	pub(super) change: &'a str,
	pub(super) authority: &'a str,
	pub(super) impact: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CommitMessageRecord {
	pub(super) schema: String,
	pub(super) change: String,
	pub(super) authority: String,
	pub(super) impact: String,
}
