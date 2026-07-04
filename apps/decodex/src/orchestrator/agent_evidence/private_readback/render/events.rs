use crate::orchestrator::agent_evidence::{
	PrivateEvidenceReadbackEvent, private_readback::render::payload,
};

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn append_private_evidence_events(
	output: &mut String,
	events: &[PrivateEvidenceReadbackEvent],
) {
	output.push_str("\nEvents\n");

	if events.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for event in events {
		output.push_str(&format!(
			"- record_id: {}\n  event_type: {}\n  recorded_at: {}\n  payload: {}\n",
			event.record_id,
			event.event_type,
			event.recorded_at,
			payload::render_private_evidence_payload_summary(&event.payload_summary)
		));

		if let Some(payload) = &event.payload {
			output.push_str(&format!("  full_payload: {}\n", payload));
		}
	}
}
