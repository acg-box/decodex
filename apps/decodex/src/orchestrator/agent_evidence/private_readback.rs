mod architecture;
mod boundary;
mod payload;
mod phase;
mod render;
mod repo_gate;
mod review;

pub(in crate::orchestrator) use self::render::render_private_evidence_readback;

use std::collections::BTreeSet;
use std::path::Path;

use crate::orchestrator::agent_evidence::{
	self, AgentPrivateEvidenceRef, EvidenceRequest, OperatorRunStatus,
	PRIVATE_EVIDENCE_READBACK_SCHEMA, PrivateEvidenceReadback, PrivateEvidenceReadbackEvent,
	PrivateEvidenceTarget, ProjectRunStatus, Result, ServiceConfig, StateStore, eyre, state,
};

pub(in crate::orchestrator) fn render_private_evidence_reference(
	run: &OperatorRunStatus,
) -> String {
	let private_evidence = agent_private_evidence_ref(run);

	format!(
		"ref={} source={} default_view={} read=`{}`",
		private_evidence.evidence_ref,
		private_evidence.source,
		private_evidence.default_view,
		private_evidence.read_command
	)
}

pub(in crate::orchestrator) fn agent_private_evidence_ref(
	run: &OperatorRunStatus,
) -> AgentPrivateEvidenceRef {
	run.private_evidence.clone()
}

pub(in crate::orchestrator) fn private_evidence_ref_for_run_fields(
	project_id: &str,
	project_config_path: &Path,
	issue_id: &str,
	issue_identifier: Option<&str>,
	run_id: &str,
	attempt_number: i64,
) -> AgentPrivateEvidenceRef {
	AgentPrivateEvidenceRef {
		evidence_ref: private_evidence_ref_for_parts(project_id, issue_id, run_id, attempt_number),
		source: String::from("runtime_sqlite"),
		default_view: String::from("summarized_payloads"),
		read_command: private_evidence_read_command(
			project_config_path,
			issue_identifier.unwrap_or(issue_id),
			Some(run_id),
			Some(attempt_number),
			true,
			false,
		),
	}
}

pub(in crate::orchestrator) fn private_evidence_ref_for_parts(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> String {
	format!("private-evidence:{project_id}/{issue_id}/{run_id}/{attempt_number}")
}

pub(in crate::orchestrator) fn shell_quote(raw: &str) -> String {
	if !raw.is_empty()
		&& raw.bytes().all(|byte| {
			byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
		}) {
		return raw.to_owned();
	}

	format!("'{}'", raw.replace('\'', "'\\''"))
}

pub(in crate::orchestrator) fn build_private_evidence_readback(
	state_store: &StateStore,
	project: &ServiceConfig,
	request: &EvidenceRequest<'_>,
) -> Result<PrivateEvidenceReadback> {
	let target = resolve_private_evidence_target(
		state_store,
		project,
		request.issue,
		request.run_id,
		request.attempt_number,
	)?;
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&target.issue_id,
		&target.run_id,
		target.attempt_number,
	)?;
	let latest_event = events.last();
	let warnings = if events.is_empty() {
		vec![String::from("private_execution_evidence_missing")]
	} else {
		Vec::new()
	};
	let issue_selector = target.issue_identifier.as_deref().unwrap_or(&target.issue_id).to_owned();
	let read_command = private_evidence_read_command(
		project.config_path(),
		&issue_selector,
		Some(&target.run_id),
		Some(target.attempt_number),
		true,
		request.include_payload,
	);

	Ok(PrivateEvidenceReadback {
		schema: PRIVATE_EVIDENCE_READBACK_SCHEMA,
		project_id: project.service_id().to_owned(),
		issue_selector: request.issue.to_owned(),
		issue_id: target.issue_id.clone(),
		issue_identifier: target.issue_identifier,
		run_id: target.run_id.clone(),
		attempt_number: target.attempt_number,
		source: "runtime_sqlite",
		evidence_ref: private_evidence_ref_for_parts(
			project.service_id(),
			&target.issue_id,
			&target.run_id,
			target.attempt_number,
		),
		read_command,
		payload_mode: if request.include_payload { "full_payloads" } else { "summarized_payloads" },
		event_count: events.len(),
		latest_event_type: latest_event.map(|event| event.event_type().to_owned()),
		latest_event_at: latest_event.map(|event| event.recorded_at().to_owned()),
		review_checkpoints: self::review::review_checkpoints_from_private_events(&events),
		repo_gate_failures: self::repo_gate::repo_gate_failures_from_private_events(&events),
		phase_acceptance_checks: self::phase::phase_acceptance_checks_from_private_events(&events),
		boundary_checks: self::boundary::boundary_checks_from_private_events(&events),
		decision_requests: self::boundary::authority_decision_requests_from_private_events(&events),
		architecture_recoveries: self::architecture::architecture_recoveries_from_private_events(
			&events,
		),
		improvement_candidates: agent_evidence::harness_improvement_candidates_from_private_events(
			&events,
		),
		events: events
			.iter()
			.map(|event| private_evidence_readback_event(event, request.include_payload))
			.collect(),
		warnings,
	})
}

fn private_evidence_read_command(
	project_config_path: &Path,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
	json: bool,
	include_payload: bool,
) -> String {
	let mut command = format!(
		"decodex evidence --config {} {}",
		shell_quote(&project_config_path.display().to_string()),
		shell_quote(issue_selector)
	);

	if let Some(run_id) = run_id {
		command.push_str(&format!(" --run-id {}", shell_quote(run_id)));
	}
	if let Some(attempt_number) = attempt_number {
		command.push_str(&format!(" --attempt {attempt_number}"));
	}

	if json {
		command.push_str(" --json");
	}
	if include_payload {
		command.push_str(" --include-payload");
	}

	command
}

fn resolve_private_evidence_target(
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

fn private_evidence_readback_event(
	event: &state::PrivateExecutionEvent,
	include_payload: bool,
) -> PrivateEvidenceReadbackEvent {
	PrivateEvidenceReadbackEvent {
		record_id: event.record_id(),
		event_type: event.event_type().to_owned(),
		recorded_at: event.recorded_at().to_owned(),
		payload_summary: self::payload::summarize_private_evidence_payload(event.payload()),
		payload: include_payload.then(|| event.payload().clone()),
	}
}
