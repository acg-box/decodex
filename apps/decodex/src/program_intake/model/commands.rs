use std::path::Path;

use crate::{
	config::ServiceConfig, state::StateStore, tracker::IssueTracker, workflow::WorkflowDocument,
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
