//! Native goal snapshots, separate from Agent coordination state.
use serde::{Deserialize, Serialize};
/// Native scheduler state; a token limit is distinct from account usage limits.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentNativeGoalStatus {
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNativeGoal {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native objective text.
	pub objective: String,
	/// True when the displayed objective omits content.
	pub objective_truncated: bool,
	/// Native scheduler state.
	pub status: AgentNativeGoalStatus,
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AgentNativeGoalResult {
	/// The native read completed for the exact requested task and thread.
	Available {
		/// Local ownership identity.
		work_id: crate::EntityId,
		/// Exact native thread, including a verified native child.
		thread_id: crate::EntityId,
		/// Time this read completed, in Unix microseconds.
		observed_at_micros: i64,
		/// Source and semantic goal identity for an explicit edit.
		#[serde(default)]
		review_token: Option<crate::WireText>,
		/// Null means the native thread has no goal.
		goal: Option<AgentNativeGoal>,
	},
	/// Native goals are disabled in the current process.
	Disabled,
	/// This native process does not implement the method.
	Unsupported,
	/// The source changed or the read could not be confirmed.
	Unavailable,
}

/// Explicit change to the native token budget.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "tokens", rename_all = "snake_case")]
pub enum AgentGoalBudgetEdit {
	/// Keep the current goal budget.
	Keep,
	/// Reset the explicit budget, retaining any configured native maximum.
	Reset,
	/// Set a positive budget, subject to native policy.
	Set(i64),
}
/// Edits to one reviewed native goal. Text can be materialized as a native attachment.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentGoalEdit {
	/// New objective; absent means preserve it.
	pub objective: Option<String>,
	/// Explicit active, paused, or complete state; absent means preserve it.
	pub status: Option<AgentNativeGoalStatus>,
	/// Explicit budget operation.
	pub budget: AgentGoalBudgetEdit,
}
