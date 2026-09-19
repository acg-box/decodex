//! Exact native task estimates; missing data is distinct from zero.
use serde::{Deserialize, Serialize};
/// Provider estimates for one exact thread; integer micros preserve precision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageEstimate {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Provider-reported estimated credits in millionths of one credit.
	pub estimated_usage_credits_micros: u64,
	/// Provider-reported estimated USD in millionths, absent when not reported.
	pub estimated_usage_usd_micros: Option<u64>,
	/// Provider groups; do not reconstruct a total from overlapping token subtotals.
	pub groups: Vec<ThreadUsageEstimateGroup>,
}

/// A model, effort and speed group, retaining missing counts as unknown.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageEstimateGroup {
	/// Provider model identifier, when reported.
	pub model: Option<String>,
	/// Provider effort, when reported.
	pub reasoning_effort: Option<String>,
	/// Provider speed, when reported.
	pub speed: Option<String>,
	/// Estimated credits in millionths.
	pub estimated_usage_credits_micros: u64,
	/// Net new input, if reported.
	pub net_new_input_tokens: Option<u64>,
	/// Cached input, if reported.
	pub cached_input_tokens: Option<u64>,
	/// Total input, including cached input when so defined by the provider.
	pub input_tokens: Option<u64>,
	/// Output, if reported.
	pub output_tokens: Option<u64>,
	/// Provider total; never synthesized from other fields.
	pub total_tokens: Option<u64>,
}

/// Source-bound observation; estimates are not account quota or a final bill.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefUsageEstimateResult {
	/// Native returned an estimate for the exact requested thread and source account.
	Available {
		/// Exact requested work.
		work_id: crate::EntityId,
		/// Local account that authenticated the request.
		account_id: crate::EntityId,
		/// Time of observation, in Unix microseconds.
		observed_at_micros: i64,
		/// Reported integer estimates and optional token groups.
		estimate: ThreadUsageEstimate,
	},
	/// Native returned no estimate (including unavailable billing route).
	NotReported,
	/// The native provider does not implement this endpoint.
	Unsupported,
	/// The complete estimate exceeds display limits.
	CapacityExceeded,
	/// Request failed or its source changed; no zero estimate is implied.
	Unavailable,
}
