use serde::Serialize;

use crate::program_intake::model::{
	GoalIntakeIssueAction, IssueBatchIntakeCounts, IssueBatchIntakeIssueReport,
};

/// Deterministic report for one issue-batch Program Intake run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct IssueBatchIntakeReport {
	/// Registered service id that owns this intake.
	pub(crate) service_id: String,
	/// Internal program id derived from the accepted issue batch.
	pub(crate) program_id: String,
	/// Whether this run was explicitly dry-run only.
	pub(crate) dry_run: bool,
	/// Whether local runtime state was persisted.
	pub(crate) persisted: bool,
	/// Deterministic classification counts.
	pub(crate) counts: IssueBatchIntakeCounts,
	/// Per-issue classification rows.
	pub(crate) issues: Vec<IssueBatchIntakeIssueReport>,
}

/// Deterministic report for one promoted-goal Program Intake run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GoalIntakeReport {
	/// Registered service id that owns this intake.
	pub(crate) service_id: String,
	/// Accepted Decision Contract that authorized the materialization.
	pub(crate) contract_id: String,
	/// Internal program id derived from the accepted goal.
	pub(crate) program_id: String,
	/// Whether this run was explicitly dry-run only.
	pub(crate) dry_run: bool,
	/// Whether Linear issues were created or updated.
	pub(crate) applied: bool,
	/// Whether local runtime Program Intake records were persisted.
	pub(crate) persisted: bool,
	/// Per-issue materialization rows.
	pub(crate) issues: Vec<GoalIntakeIssueReport>,
}

/// Per-issue promoted-goal materialization row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct GoalIntakeIssueReport {
	/// Internal Execution Program node id.
	pub(crate) node_id: String,
	/// Public issue title.
	pub(crate) title: String,
	/// Natural-language objective for this generated issue.
	pub(crate) objective: String,
	/// Linear issue id after apply, or the linked id found during dry-run.
	pub(crate) issue_id: Option<String>,
	/// Linear issue identifier after apply, or the linked identifier found during dry-run.
	pub(crate) issue_identifier: Option<String>,
	/// Whether apply would create/update or did create/update the issue.
	pub(crate) action: GoalIntakeIssueAction,
	/// Queue intent stored on the internal program node.
	pub(crate) queue_intent: String,
	/// Direct dispatch action derived for the mapped node, when known.
	pub(crate) dispatch_action: Option<String>,
	/// Dependency ids or public issue identifiers required before this node can run.
	pub(crate) dependencies: Vec<String>,
	/// Coarse conflict domains retained in the internal program.
	pub(crate) conflict_domains: Vec<String>,
	/// Acceptance expectations rendered into the public issue brief.
	pub(crate) acceptance: Vec<String>,
	/// Validation expectations rendered into the public issue brief.
	pub(crate) validation: Vec<String>,
	/// Deterministic local readback reasons.
	pub(crate) reasons: Vec<String>,
}
