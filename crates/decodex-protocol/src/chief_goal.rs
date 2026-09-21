//! Read-only native goal evidence; Codex owns persistence and continuation.
use crate::EntityId;
use serde::{Deserialize, Serialize};

/// One goal read from the selected native thread.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChiefNativeGoal {
	/// Exact native thread identity.
	pub thread_id: String,
	/// User-authored objective.
	pub objective: String,
	/// Native status, including bounded future values.
	pub status: String,
	/// Optional native token limit.
	pub token_budget: Option<u64>,
	/// Tokens accounted by the native goal owner.
	pub tokens_used: u64,
	/// Elapsed execution seconds accounted by the native goal owner.
	pub time_used_seconds: u64,
	/// Native creation timestamp in Unix seconds.
	pub created_at: i64,
	/// Native update timestamp in Unix seconds.
	pub updated_at: i64,
}
impl ChiefNativeGoal {
	/// Validate bounds without inferring goal completion from local work status.
	pub fn is_valid(&self) -> bool {
		!self.thread_id.is_empty()
			&& self.thread_id.len() <= 512
			&& !self.objective.trim().is_empty()
			&& self.objective.len() <= 65536
			&& !self.status.is_empty()
			&& self.status.len() <= 64
			&& !self.status.chars().any(char::is_control)
			&& self.created_at >= 0
			&& self.updated_at >= self.created_at
			&& self.token_budget.is_none_or(|budget| budget > 0)
	}
}

/// Fresh readback for the current source; unavailable is not an empty goal.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefGoalResult {
	/// Native read completed for this exact source and thread.
	Available {
		/// Account, process and history source identity.
		source: EntityId,
		/// Exact native binding, also present when no goal exists.
		thread_id: String,
		/// None means a successful native read found no goal.
		goal: Option<ChiefNativeGoal>,
	},
	/// The work has no native thread.
	Unbound,
	/// This native server has no enabled goal API.
	Unsupported,
	/// Source or readback could not be confirmed.
	Unavailable,
}
impl ChiefGoalResult {
	/// Validate wire evidence and its thread relationship.
	pub fn is_valid(&self) -> bool {
		match self {
			Self::Available { thread_id, goal, .. } =>
				!thread_id.is_empty()
					&& thread_id.len() <= 512
					&& goal
						.as_ref()
						.is_none_or(|goal| goal.is_valid() && goal.thread_id == *thread_id),
			_ => true,
		}
	}
}
