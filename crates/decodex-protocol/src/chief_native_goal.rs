//! Native goal snapshots, separate from Chief coordination state.
use serde::{Deserialize, Serialize};
/// Native scheduler state; a token limit is distinct from account usage limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChiefNativeGoalStatus {
	/// The native goal can continue work.
	Active,
	/// The native goal is paused.
	Paused,
	/// The native goal needs external progress.
	Blocked,
	/// Account usage prevents progress.
	UsageLimited,
	/// The explicit goal budget prevents progress.
	BudgetLimited,
	/// The native goal has completed.
	Complete,
}

/// One native goal snapshot; counters belong to this goal, not all thread history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChiefNativeGoal {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native objective text.
	pub objective: String,
	/// True when the displayed objective omits content.
	pub objective_truncated: bool,
	/// Native scheduler state.
	pub status: ChiefNativeGoalStatus,
	/// Explicit token budget; null means no configured budget.
	pub token_budget: Option<i64>,
	/// Native goal token counter.
	pub tokens_used: i64,
	/// Native goal elapsed time in seconds.
	pub time_used_seconds: i64,
	/// Native creation timestamp in Unix seconds.
	pub created_at: i64,
	/// Native last-update timestamp in Unix seconds.
	pub updated_at: i64,
}

/// One source-bound native goal read. Failure never means no goal exists.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefNativeGoalResult {
	/// The native read completed for the exact requested task and thread.
	Available {
		/// Local ownership identity.
		work_id: crate::EntityId,
		/// Exact native thread, including a verified native child.
		thread_id: crate::EntityId,
		/// Time this read completed, in Unix microseconds.
		observed_at_micros: i64,
		/// Null means the native thread has no goal.
		goal: Option<ChiefNativeGoal>,
	},
	/// Native goals are disabled in the current process.
	Disabled,
	/// This native process does not implement the method.
	Unsupported,
	/// The source changed or the read could not be confirmed.
	Unavailable,
}
