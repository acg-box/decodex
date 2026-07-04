use serde::Serialize;

use crate::program_intake::model::IssueBatchIntakeClassification;

/// Count summary for an issue-batch intake report.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeCounts {
	/// Issues ready for queue intent.
	pub(crate) ready: usize,
	/// Issues intentionally held from queueing.
	pub(crate) held: usize,
	/// Issues blocked by dependencies, attention, or briefing.
	pub(crate) blocked: usize,
	/// Issues that are stale or terminal for the accepted batch.
	pub(crate) stale: usize,
	/// Supplied identifiers that did not map to Linear issues.
	pub(crate) unmapped: usize,
}

/// Per-issue report row for issue-batch intake.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeIssueReport {
	/// Linear issue identifier supplied by the operator.
	pub(crate) issue_identifier: String,
	/// Linear issue id, when the identifier resolved.
	pub(crate) issue_id: Option<String>,
	/// Current Linear workflow state, when the identifier resolved.
	pub(crate) issue_state: Option<String>,
	/// Normalized intake classification.
	pub(crate) classification: IssueBatchIntakeClassification,
	/// Queue intent stored on the internal program node, when available.
	pub(crate) queue_intent: Option<String>,
	/// Readiness-derived direct dispatch action.
	pub(crate) dispatch_action: Option<String>,
	/// Deterministic local readback reasons.
	pub(crate) reasons: Vec<String>,
	/// Known blocker issue identifiers.
	pub(crate) blockers: Vec<String>,
	/// Coarse conflict-domain hints.
	pub(crate) conflict_domains: Vec<String>,
}
