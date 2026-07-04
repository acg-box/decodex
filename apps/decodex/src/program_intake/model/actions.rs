use serde::{Deserialize, Serialize};

/// Normalized issue-batch intake classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum IssueBatchIntakeClassification {
	/// Ready for later queue-label reconciliation.
	Ready,
	/// Intentionally held from queueing.
	Held,
	/// Blocked by issue state, dependency, attention, or briefing evidence.
	Blocked,
	/// Terminal or stale relative to the accepted intake boundary.
	Stale,
	/// Supplied identifier did not map to a tracker issue.
	Unmapped,
}
impl IssueBatchIntakeClassification {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Ready => "ready",
			Self::Held => "held",
			Self::Blocked => "blocked",
			Self::Stale => "stale",
			Self::Unmapped => "unmapped",
		}
	}
}

/// Promoted-goal materialization action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GoalIntakeIssueAction {
	/// Dry-run would create a new normal Linear issue.
	WouldCreate,
	/// Dry-run would update an already linked normal Linear issue.
	WouldUpdate,
	/// Apply created a new normal Linear issue.
	Created,
	/// Apply updated an already linked normal Linear issue.
	Updated,
}
impl GoalIntakeIssueAction {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::WouldCreate => "would_create",
			Self::WouldUpdate => "would_update",
			Self::Created => "created",
			Self::Updated => "updated",
		}
	}
}
