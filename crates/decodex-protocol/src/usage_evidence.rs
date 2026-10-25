//! Actual retained native counters. These values are not billing estimates.
use serde::{Deserialize, Serialize};

use crate::EntityId;

/// Explicit product scope; selection does not assign a conversation turn to Work.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UsageEvidenceScope {
	/// All exact retained receipts for one persistent Agent, across its bindings.
	Agent {
		/// Persistent Agent identity.
		agent_id: EntityId,
	},
	/// Exact Result, native delivery or peer instruction references for one Work.
	Work {
		/// Business Work identity.
		work_id: EntityId,
	},
}
impl UsageEvidenceScope {
	/// The selected product identity.
	pub fn id(&self) -> &EntityId {
		match self {
			Self::Agent { agent_id } => agent_id,
			Self::Work { work_id } => work_id,
		}
	}
}

/// A distinct native turn and its explicit retained attribution.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsageEvidenceSource {
	/// Terminal receipt, absent when only an assignment or Result reference is known.
	pub event_id: Option<i64>,
	/// Persistent Agent responsible for this observation.
	pub agent_id: String,
	/// Exact native conversation; native children retain their own identity.
	pub thread_id: String,
	/// Exact native turn.
	pub turn_id: String,
	/// Up to four exact Work references; count can be larger.
	pub work_ids: Vec<String>,
	/// Total distinct Work references to this turn.
	pub work_count: u64,
	/// Whole-turn input tokens, including cached input, when known.
	pub input_tokens: Option<u64>,
	/// Whole-turn output tokens, including reasoning, when known.
	pub output_tokens: Option<u64>,
	/// Recorded native turn duration; absent when not retained.
	pub duration_ms: Option<u64>,
}

/// Known totals plus their measured coverage. Zero measured turns means unknown totals.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UsageEvidenceTotals {
	/// Distinct retained turn identities within the scope.
	pub turns: u64,
	/// Turns included in token totals, excluding shared Work turns.
	pub token_turns: u64,
	/// Turns included in recorded duration, excluding shared Work turns.
	pub duration_turns: u64,
	/// Turns referenced by more than one Work; excluded from Work totals.
	pub shared_turns: u64,
	/// Agent turns without an explicit Work attribution.
	pub unattributed_turns: u64,
	/// Sum of known whole-turn input tokens.
	pub input_tokens: u64,
	/// Sum of known whole-turn output tokens.
	pub output_tokens: u64,
	/// Sum of native turn durations. Parallel duration is not Work wall-clock time.
	pub duration_ms: u64,
}

/// Read-only coverage of retained native receipts and explicit Work attribution.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum UsageEvidenceResult {
	/// Current source or requested product identity is unavailable.
	Unavailable,
	/// Aggregate over all retained matching receipts, with bounded source examples.
	Available {
		/// Current runtime source used to guard the read.
		source: EntityId,
		/// Exact requested scope.
		scope: UsageEvidenceScope,
		/// Observed totals and coverage.
		totals: UsageEvidenceTotals,
		/// At most 32 source examples, independently bounded by transport size.
		sources: Vec<UsageEvidenceSource>,
		/// Some exact sources are not included in this preview; aggregate coverage is unchanged.
		sources_truncated: bool,
	},
}
