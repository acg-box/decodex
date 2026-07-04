use crate::{
	orchestrator::agent_evidence::{PrivateEvidenceReadbackEvent, private_readback::payload},
	state::PrivateExecutionEvent,
};

pub(crate) fn private_evidence_readback_event(
	event: &PrivateExecutionEvent,
	include_payload: bool,
) -> PrivateEvidenceReadbackEvent {
	PrivateEvidenceReadbackEvent {
		record_id: event.record_id(),
		event_type: event.event_type().to_owned(),
		recorded_at: event.recorded_at().to_owned(),
		payload_summary: payload::summarize_private_evidence_payload(event.payload()),
		payload: include_payload.then(|| event.payload().clone()),
	}
}
