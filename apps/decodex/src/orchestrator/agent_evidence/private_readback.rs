mod architecture;
mod boundary;
mod events;
mod payload;
mod phase;
mod reference;
mod render;
mod repo_gate;
mod review;
mod target;

pub(crate) use self::{
	reference::{
		agent_private_evidence_ref, private_evidence_ref_for_run_fields,
		render_private_evidence_reference,
	},
	render::render_private_evidence_readback,
};

use crate::orchestrator::agent_evidence::{
	self, EvidenceRequest, PRIVATE_EVIDENCE_READBACK_SCHEMA, PrivateEvidenceReadback, Result,
	ServiceConfig, StateStore,
};

pub(crate) fn build_private_evidence_readback(
	state_store: &StateStore,
	project: &ServiceConfig,
	request: &EvidenceRequest<'_>,
) -> Result<PrivateEvidenceReadback> {
	let target = target::resolve_private_evidence_target(
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
	let read_command = reference::private_evidence_read_command(
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
		evidence_ref: reference::private_evidence_ref_for_parts(
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
		validation_evidence: self::phase::validation_evidence_from_private_events(&events),
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
			.map(|event| events::private_evidence_readback_event(event, request.include_payload))
			.collect(),
		warnings,
	})
}
