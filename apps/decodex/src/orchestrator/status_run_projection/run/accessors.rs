use crate::orchestrator::agent_evidence;
use crate::orchestrator::git_ops;
use crate::orchestrator::status;
use crate::orchestrator::status_run_projection::loop_status;
use crate::orchestrator::{
	AgentPrivateEvidenceRef, CodexAccountActivitySummary, OperatorLoopStatus,
	ProjectLoopEvidenceSnapshot, ProjectRunStatus, RunActivityMarker, ServiceConfig,
};
use crate::prelude::Result;

pub(super) fn operator_run_accounts(
	marker: Option<&RunActivityMarker>,
) -> (Option<CodexAccountActivitySummary>, Vec<CodexAccountActivitySummary>) {
	let account = marker.and_then(RunActivityMarker::account).cloned();
	let mut accounts = marker.map(|marker| marker.accounts().to_vec()).unwrap_or_default();

	status::append_primary_account_if_missing(&mut accounts, account.as_ref());

	(account, accounts)
}

pub(super) fn operator_run_relative_worktree_path(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
) -> Option<String> {
	run.worktree_path().map(|path| git_ops::relative_worktree_path_for_path(project, path))
}

pub(super) fn operator_run_private_evidence(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> AgentPrivateEvidenceRef {
	agent_evidence::private_evidence_ref_for_run_fields(
		project.service_id(),
		project.config_path(),
		run.issue_id(),
		issue_identifier,
		run.run_id(),
		run.attempt_number(),
	)
}

pub(super) fn operator_run_loop_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	run: &ProjectRunStatus,
	status: &str,
	phase: &str,
	current_operation: &str,
) -> Result<OperatorLoopStatus> {
	loop_status::operator_loop_status_for_run_with_evidence(
		project,
		loop_evidence,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
		super::operator_run_default_review_phase(status, phase, current_operation),
		super::operator_run_lifecycle_loop_summary(status, phase, current_operation),
	)
}
