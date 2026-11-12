//! Exact native task estimates; missing data is distinct from zero.
use serde::{Deserialize, Serialize};

use crate::EntityId;

/// Provider estimates for one exact thread; integer micros preserve precision.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUsageEstimate {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Provider-reported estimated credits in millionths of one credit.
	pub estimated_usage_credits_micros: u64,
	/// Provider-reported estimated USD in millionths, absent when not reported.
	pub estimated_usage_usd_micros: Option<u64>,
	/// Native dollar usage, independent of credit estimates. None means unknown.
	pub native_usage_usd_micros: Option<u64>,
	/// Provider groups; do not reconstruct a total from overlapping token subtotals.
	pub groups: Vec<ThreadUsageEstimateGroup>,
}

impl ThreadUsageEstimate {
	/// Prefer native dollars. Use the provider's dollar estimate only when native dollars are
	/// absent.
	pub fn reported_usd_micros(&self) -> Option<u64> {
		self.native_usage_usd_micros.or(self.estimated_usage_usd_micros)
	}
}

/// A model, effort and speed group, retaining missing counts as unknown.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentUsageEstimateResult {
	/// Native returned an estimate for the exact requested thread and source account.
	Available {
		/// Exact requested work.
		work_id: EntityId,
		/// Local account that authenticated the request.
		account_id: EntityId,
		/// Time of observation, in Unix microseconds.
		observed_at_micros: i64,
		/// Reported integer estimates and optional token groups.
		estimate: ThreadUsageEstimate,
	},
	/// Public API price equivalent of observed tokens, not subscription billing.
	ApiEstimate {
		/// Exact requested work.
		work_id: EntityId,
		/// Account that owns the observed conversation.
		account_id: EntityId,
		/// Time of the local calculation.
		observed_at_micros: i64,
		/// Token-only estimate with explicit coverage and price age.
		estimate: ApiCostEstimate,
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

/// API-equivalent text-token cost. It excludes tool charges and subscription fees.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ApiCostEstimate {
	/// Exact native conversation.
	pub thread_id: String,
	/// Estimated USD in millionths.
	pub usd_micros: u64,
	/// Number of observed model responses included in the estimate.
	pub priced_responses: u64,
	/// Some earlier usage, models, speeds or pricing fields could not be priced.
	pub partial: bool,
	/// Last successful check of the official price table, in Unix microseconds.
	pub pricing_checked_at_micros: i64,
}
