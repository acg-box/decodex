use crate::orchestrator::{self, AgentEvidenceSource, OperatorStatusSnapshot};

pub(crate) fn write_snapshot_evidence(snapshot: &OperatorStatusSnapshot) {
	orchestrator::write_agent_evidence_best_effort(snapshot, AgentEvidenceSource::ServeTick);
}
