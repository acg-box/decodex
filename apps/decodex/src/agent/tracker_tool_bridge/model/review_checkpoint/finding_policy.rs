use serde::{Deserialize, Serialize};

use crate::agent::tracker_tool_bridge::model::review_checkpoint::args::ReviewCheckpointLineRangeArgs;

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ReviewFindingPolicyState {
	pub(crate) schema: String,
	pub(crate) phase: String,
	pub(crate) status: String,
	pub(crate) head_sha: String,
	pub(crate) nonclean_rounds: i64,
	pub(crate) active_fingerprints: Vec<String>,
	pub(crate) stop_fingerprint: Option<String>,
	pub(crate) findings: Vec<ReviewFindingPolicyRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ReviewFindingPolicyRecord {
	pub(crate) fingerprint: String,
	pub(crate) kind: String,
	pub(crate) title: String,
	pub(crate) body: String,
	pub(crate) file: Option<String>,
	pub(crate) line_range: Option<ReviewCheckpointLineRangeArgs>,
	pub(crate) first_seen_head: String,
	pub(crate) last_seen_head: String,
	pub(crate) status: String,
	pub(crate) repeat_count: i64,
	pub(crate) repair_evidence: Vec<String>,
}
