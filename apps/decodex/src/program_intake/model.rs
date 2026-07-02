use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{
	config::ServiceConfig,
	execution_program::{ExecutionConflictDomain, ExecutionProgramNodeStage, ExecutionQueueIntent},
	loop_contract::DecisionContract,
	state::StateStore,
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};

/// CLI/runtime request for issue-batch Program Intake.
pub(crate) struct IssueBatchIntakeCommandRequest<'a> {
	pub(crate) config_path: Option<&'a Path>,
	pub(crate) project_id: Option<&'a str>,
	pub(crate) issue_identifiers: Vec<String>,
	pub(crate) dry_run: bool,
	pub(crate) persist: bool,
}

/// CLI/runtime request for promoted-goal Program Intake.
pub(crate) struct GoalIntakeCommandRequest<'a> {
	/// Project config override, if supplied.
	pub(crate) config_path: Option<&'a Path>,
	/// Registered service id to intake against.
	pub(crate) project_id: Option<&'a str>,
	/// Accepted Decision Contract id to materialize.
	pub(crate) contract_id: &'a str,
	/// Optional Linear issue whose team/startable state should anchor generated issues.
	pub(crate) team_issue_identifier: Option<&'a str>,
	/// Read and render the proposed materialization without mutating Linear or local intake rows.
	pub(crate) dry_run: bool,
	/// Create or update generated Linear issues and persist the Execution Program.
	pub(crate) apply: bool,
}

/// In-process request for promoted-goal Program Intake.
pub(crate) struct GoalIntakeRunRequest<'a, T>
where
	T: IssueTracker + ?Sized,
{
	/// Runtime state store used for Decision Contract and program persistence.
	pub(crate) state_store: &'a StateStore,
	/// Tracker adapter used for Linear reads and apply-mode writes.
	pub(crate) tracker: &'a T,
	/// Registered service config that owns this intake.
	pub(crate) config: &'a ServiceConfig,
	/// Registered workflow document for queue and startable-state policy.
	pub(crate) workflow: &'a WorkflowDocument,
	/// Accepted Decision Contract id to materialize.
	pub(crate) contract_id: &'a str,
	/// Optional Linear issue whose team/startable state should anchor generated issues.
	pub(crate) team_issue_identifier: Option<String>,
	/// Read and render without mutation.
	pub(crate) dry_run: bool,
	/// Create/update Linear issues and persist local intake rows.
	pub(crate) apply: bool,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IssueFacts {
	pub(crate) has_active_label: bool,
	pub(crate) has_opt_out_label: bool,
	pub(crate) has_needs_attention_label: bool,
	pub(crate) has_generic_dispatch_briefing: bool,
	pub(crate) has_open_blockers: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GoalIssuePlan {
	pub(crate) key: String,
	pub(crate) node_id: String,
	pub(crate) title: String,
	pub(crate) objective: String,
	pub(crate) stage: ExecutionProgramNodeStage,
	pub(crate) queue_intent: ExecutionQueueIntent,
	pub(crate) description: String,
	pub(crate) dependencies: Vec<String>,
	pub(crate) dependency_node_ids: Vec<String>,
	pub(crate) conflict_domains: Vec<ExecutionConflictDomain>,
	pub(crate) acceptance: Vec<String>,
	pub(crate) validation: Vec<String>,
	pub(crate) risk: Vec<String>,
}

pub(crate) struct GoalIntakeAnchor {
	pub(crate) team_id: String,
	pub(crate) state_id: String,
}

pub(crate) struct GoalIssueBriefInput<'a> {
	pub(crate) contract: &'a DecisionContract,
	pub(crate) objective: &'a str,
	pub(crate) dependencies: &'a [String],
	pub(crate) conflict_domains: &'a [ExecutionConflictDomain],
	pub(crate) acceptance: &'a [String],
	pub(crate) validation: &'a [String],
	pub(crate) risk: &'a [String],
}

pub(crate) struct ApplyGoalIssuesInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	pub(crate) state_store: &'a StateStore,
	pub(crate) service_id: &'a str,
	pub(crate) source_issue_id: Option<&'a str>,
	pub(crate) tracker: &'a T,
	pub(crate) contract: &'a DecisionContract,
	pub(crate) plans: &'a [GoalIssuePlan],
	pub(crate) linked_issues: &'a [Option<TrackerIssue>],
	pub(crate) anchor: &'a GoalIntakeAnchor,
}

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
