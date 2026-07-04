use std::collections::BTreeSet;

use crate::{
	orchestrator::agent_evidence::{
		self, PrivateEvidenceTarget, ProjectRunStatus, Result, ServiceConfig, StateStore, eyre,
	},
	state,
};

pub(crate) fn resolve_private_evidence_target(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
) -> Result<PrivateEvidenceTarget> {
	let (_, runs) = state_store.list_project_runs(project.service_id(), usize::MAX)?;
	let selector = issue_selector.trim();
	let matching_run = runs
		.iter()
		.filter(|run| private_evidence_run_matches_issue(project, run, selector))
		.filter(|run| run_id.is_none_or(|run_id| run.run_id() == run_id))
		.find(|run| attempt_number.is_none_or(|attempt| run.attempt_number() == attempt));

	if let Some(run) = matching_run {
		let branch_name = run.branch_name().map(str::to_owned);
		let worktree_path = run
			.worktree_path()
			.map(|path| agent_evidence::relative_worktree_path_for_path(project, path));
		let issue_identifier = agent_evidence::operator_run_issue_identifier_from_fields(
			run.run_id(),
			branch_name.as_deref(),
			worktree_path.as_deref(),
		);

		return Ok(PrivateEvidenceTarget {
			issue_id: run.issue_id().to_owned(),
			issue_identifier,
			run_id: run.run_id().to_owned(),
			attempt_number: run.attempt_number(),
		});
	}
	if let (Some(run_id), Some(attempt_number)) = (run_id, attempt_number) {
		let events = state_store.list_private_execution_events_for_run_attempt(
			project.service_id(),
			run_id,
			attempt_number,
		)?;

		if let Some(issue_id) = private_evidence_direct_lookup_issue_id(&events, selector)? {
			return Ok(PrivateEvidenceTarget {
				issue_identifier: (issue_id != selector).then(|| selector.to_owned()),
				issue_id,
				run_id: run_id.to_owned(),
				attempt_number,
			});
		}

		return Ok(PrivateEvidenceTarget {
			issue_id: selector.to_owned(),
			issue_identifier: None,
			run_id: run_id.to_owned(),
			attempt_number,
		});
	}

	eyre::bail!(
		"No local run matched issue `{selector}` in project `{}`. Pass --run-id and --attempt for direct runtime-store lookup, or run `decodex status --json` to find local run ids.",
		project.service_id()
	)
}

fn private_evidence_direct_lookup_issue_id(
	events: &[state::PrivateExecutionEvent],
	selector: &str,
) -> Result<Option<String>> {
	let issue_ids =
		events.iter().map(state::PrivateExecutionEvent::issue_id).collect::<BTreeSet<_>>();

	if issue_ids.is_empty() {
		return Ok(None);
	}
	if issue_ids.len() == 1 {
		return Ok(issue_ids.iter().next().map(|issue_id| (*issue_id).to_owned()));
	}
	if issue_ids.contains(selector) {
		return Ok(Some(selector.to_owned()));
	}

	eyre::bail!(
		"Direct private evidence lookup for issue `{selector}` matched multiple local issue ids for the supplied run and attempt; pass the local issue id from `decodex status --json`."
	)
}

fn private_evidence_run_matches_issue(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	selector: &str,
) -> bool {
	if run.issue_id() == selector {
		return true;
	}

	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path = run
		.worktree_path()
		.map(|path| agent_evidence::relative_worktree_path_for_path(project, path));
	let issue_identifier = agent_evidence::operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);

	issue_identifier
		.as_deref()
		.is_some_and(|issue_identifier| issue_identifier.eq_ignore_ascii_case(selector))
}
