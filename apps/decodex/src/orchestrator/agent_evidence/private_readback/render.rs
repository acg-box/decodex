mod events;
mod header;
mod payload;
mod recovery;
mod review;

use crate::orchestrator::agent_evidence::PrivateEvidenceReadback;

pub(crate) fn render_private_evidence_readback(readback: &PrivateEvidenceReadback) -> String {
	let mut output = String::new();

	header::append_private_evidence_readback_header(&mut output, readback);
	review::append_private_evidence_decision_requests(&mut output, &readback.decision_requests);
	review::append_private_evidence_review_checkpoints(&mut output, &readback.review_checkpoints);
	review::append_private_evidence_repo_gate_failures(&mut output, &readback.repo_gate_failures);
	review::append_private_evidence_validation_evidence(&mut output, &readback.validation_evidence);
	recovery::append_private_evidence_architecture_recoveries(
		&mut output,
		&readback.architecture_recoveries,
	);
	recovery::append_private_evidence_boundary_checks(&mut output, &readback.boundary_checks);
	recovery::append_private_evidence_improvement_candidates(
		&mut output,
		&readback.improvement_candidates,
	);
	events::append_private_evidence_events(&mut output, &readback.events);

	output
}
