mod capsules;
mod files;
mod models;
mod paths;
mod private_readback;
mod project_view;
mod snapshot;

pub(crate) use self::{
	models::{
		AgentEvidenceSource, AgentEvidenceSummary, AgentEvidenceWriteResult,
		AgentPrivateEvidenceRef, PrivateEvidenceArchitectureRecoverySummary,
		PrivateEvidenceBoundaryCheckSummary, PrivateEvidenceDecisionRequestSummary,
		PrivateEvidencePayloadSummary, PrivateEvidencePhaseAcceptanceSummary,
		PrivateEvidenceReadback, PrivateEvidenceReadbackEvent,
		PrivateEvidenceRepoGateFailureSummary, PrivateEvidenceReviewCheckpointSummary,
		PrivateEvidenceReviewRouteCount,
	},
	paths::{
		blocker_evidence_ref, blocker_snapshot_path, current_month_bucket, issue_key,
		run_capsule_path, run_evidence_ref, sanitize_evidence_path_component,
	},
	private_readback::{
		agent_private_evidence_ref, build_private_evidence_readback,
		private_evidence_ref_for_run_fields, render_private_evidence_readback,
		render_private_evidence_reference,
	},
	project_view::{AgentEvidenceProjectView, agent_evidence_project_ids},
	snapshot::{
		render_agent_evidence_write_result, write_agent_evidence_best_effort,
		write_agent_evidence_snapshot,
	},
};

use self::{
	capsules::{
		agent_connector_backoff, agent_recovery_contract, agent_recovery_worktree,
		build_agent_blockers, build_run_capsules, run_capsule_ref,
	},
	files::write_agent_evidence_files,
	models::{
		AgentBlocker, AgentBlockerSnapshot, AgentConnectorBackoff, AgentEvidenceEvent,
		AgentEvidenceFileWriteContext, AgentHandoffIndex, AgentRecoveryContract,
		AgentRecoveryWorktree, AgentRunCapsule, AgentRunCapsuleRef, AgentRunDiagnosis,
		AgentRunLedgerOutcome, PrivateEvidenceTarget,
	},
};
use crate::{
	orchestrator::{
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		EvidenceRequest, OperatorRunStatus, OperatorStatusSnapshot,
		PHASE_ACCEPTANCE_CHECK_EVENT_TYPE, ProjectRunStatus, ServiceConfig, StateStore,
		current_timestamp, harness_improvement::harness_improvement_candidates_from_private_events,
		operator_run_issue_identifier_from_fields, relative_worktree_path_for_path,
	},
	prelude::{Result, eyre},
	runtime,
};

const AGENT_HANDOFF_INDEX_SCHEMA: &str = "decodex.agent_handoff_index/1";
const AGENT_BLOCKER_SNAPSHOT_SCHEMA: &str = "decodex.blocker_snapshot/1";
const AGENT_RUN_CAPSULE_SCHEMA: &str = "decodex.run_capsule/1";
const AGENT_EVIDENCE_EVENT_SCHEMA: &str = "decodex.agent_evidence_event/1";
const PRIVATE_EVIDENCE_READBACK_SCHEMA: &str = "decodex.private_execution_evidence_readback/1";
const PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT: usize = 160;
const REVIEW_CHECKPOINT_EVENT_TYPE: &str = "review_checkpoint";
const HANDOFF_INDEX_FILE_NAME: &str = "handoff-index.json";
const BLOCKERS_DIR_NAME: &str = "blockers";
const RUNS_DIR_NAME: &str = "runs";
const EVENTS_FILE_NAME: &str = "events.jsonl";
