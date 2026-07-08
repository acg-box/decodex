mod surfaces;
mod transitions;
mod types;

pub(crate) use self::{
	surfaces::{
		validation_evidence_blocker_count,
		validation_evidence_blockers_resolved_by_validation_repair,
		validation_evidence_changed_surfaces, validation_evidence_docs_impact_valid,
		validation_evidence_has_non_goal_violation,
	},
	transitions::{
		phase_terminal_goal_complete_signal, phase_tracked_rewrite_handoff_detail,
		phase_validation_pass_next_phase, validation_evidence_reason_code,
		validation_evidence_repair_phase,
	},
	types::{ValidationDecision, ValidationEvidence, ValidationEvidenceFailure},
};
