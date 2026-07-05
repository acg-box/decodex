use crate::orchestrator::agent_evidence::{AgentRunCapsule, AgentRunCapsuleRef};

pub(crate) fn run_capsule_ref(capsule: &AgentRunCapsule) -> AgentRunCapsuleRef {
	AgentRunCapsuleRef {
		evidence_ref: capsule.evidence_ref.clone(),
		run_id: capsule.run_id.clone(),
		issue_id: capsule.issue_id.clone(),
		issue_identifier: capsule.issue_identifier.clone(),
		attempt_number: capsule.attempt_number,
		status: capsule.status.clone(),
		phase: capsule.phase.clone(),
		current_operation: capsule.current_operation.clone(),
		path: capsule.path.clone(),
		private_evidence: capsule.private_evidence.clone(),
	}
}
