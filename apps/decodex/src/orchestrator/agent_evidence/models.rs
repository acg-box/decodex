mod handoff;
mod private_readback;
mod run_capsule;

pub(crate) use self::{
	handoff::{
		AgentBlocker, AgentBlockerSnapshot, AgentConnectorBackoff, AgentEvidenceEvent,
		AgentEvidenceFileWriteContext, AgentEvidenceSummary, AgentEvidenceWriteResult,
		AgentHandoffIndex, AgentRecoveryContract, AgentRecoveryWorktree, PrivateEvidenceTarget,
	},
	private_readback::{
		PrivateEvidenceArchitectureRecoverySummary, PrivateEvidenceBoundaryCheckSummary,
		PrivateEvidenceDecisionRequestSummary, PrivateEvidencePayloadSummary,
		PrivateEvidenceReadback, PrivateEvidenceReadbackEvent,
		PrivateEvidenceRepoGateFailureSummary, PrivateEvidenceReviewCheckpointSummary,
		PrivateEvidenceReviewRouteCount, PrivateEvidenceValidationSummary,
	},
	run_capsule::{AgentRunCapsule, AgentRunCapsuleRef, AgentRunDiagnosis, AgentRunLedgerOutcome},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentEvidenceSource {
	DiagnoseCommand,
	ServeTick,
}
impl AgentEvidenceSource {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::DiagnoseCommand => "diagnose_command",
			Self::ServeTick => "serve_tick",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AgentPrivateEvidenceRef {
	pub(crate) evidence_ref: String,
	pub(crate) source: String,
	pub(crate) default_view: String,
	pub(crate) read_command: String,
}
