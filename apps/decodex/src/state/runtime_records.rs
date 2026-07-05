use std::path::PathBuf;

use serde_json::Value;

use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::AutonomySignal,
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
	state::{
		self, AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
		ChildAgentActivitySummary, DecisionContractRecord, LoopGuardrailCheckpoint,
		PrivateExecutionEvent, ProtocolActivitySummary, ReviewHandoffMarker, ReviewLifecycleRecord,
		ReviewPolicyCheckpoint, RunAttempt, RunControlChannel, WorktreeMapping,
	},
	tracker::records::LinearExecutionEventRecord,
};

pub(in crate::state) struct TimestampParts {
	pub(in crate::state) text: String,
	pub(in crate::state) unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunAttemptRecord {
	pub(in crate::state) run_id: String,
	pub(in crate::state) project_id: Option<String>,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) status: String,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl RunAttemptRecord {
	pub(in crate::state) fn as_public(&self) -> RunAttempt {
		RunAttempt {
			run_id: self.run_id.clone(),
			issue_id: self.issue_id.clone(),
			attempt_number: self.attempt_number,
			status: self.status.clone(),
			thread_id: self.thread_id.clone(),
			turn_id: self.turn_id.clone(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunControlChannelRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) transport: String,
	pub(in crate::state) channel_path: PathBuf,
	pub(in crate::state) status: String,
	pub(in crate::state) published_at: String,
	pub(in crate::state) published_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl RunControlChannelRecord {
	pub(in crate::state) fn as_public(&self) -> RunControlChannel {
		RunControlChannel {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			transport: self.transport.clone(),
			channel_path: self.channel_path.clone(),
			status: self.status.clone(),
			published_at: self.published_at.clone(),
			published_at_unix: self.published_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ProtocolEventRecord {
	pub(in crate::state) sequence_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload_sha256: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
}
impl ProtocolEventRecord {
	pub(in crate::state) fn is_idempotent_replay_of(&self, candidate: &Self) -> bool {
		self.event_type == candidate.event_type && self.payload_sha256 == candidate.payload_sha256
	}
}

#[derive(Clone, Debug, Default)]
pub(in crate::state) struct ProtocolEventSummaryRecord {
	pub(in crate::state) event_count: i64,
	pub(in crate::state) last_sequence_number: Option<i64>,
	pub(in crate::state) last_event_type: Option<String>,
	pub(in crate::state) last_event_at: Option<String>,
	pub(in crate::state) last_event_at_unix: Option<i64>,
}
impl ProtocolEventSummaryRecord {
	pub(in crate::state) fn record_event(&mut self, event: &ProtocolEventRecord) {
		self.event_count += 1;

		if self
			.last_sequence_number
			.is_none_or(|sequence_number| event.sequence_number >= sequence_number)
		{
			self.last_sequence_number = Some(event.sequence_number);
			self.last_event_type = Some(event.event_type.clone());
			self.last_event_at = Some(event.created_at.clone());
			self.last_event_at_unix = Some(event.created_at_unix);
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunActivitySummaryRecord {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(in crate::state) protocol_activity: Option<ProtocolActivitySummary>,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomyObjectiveKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) version: u64,
}
impl AutonomyObjectiveKey {
	pub(in crate::state) fn new(project_id: &str, objective_id: &str, version: u64) -> Self {
		Self { project_id: project_id.to_owned(), objective_id: objective_id.to_owned(), version }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomyObjectiveRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective: AutonomyObjectiveContract,
	pub(in crate::state) state: AutonomyObjectiveState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomyObjectiveRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> AutonomyObjectiveKey {
		AutonomyObjectiveKey::new(&self.project_id, self.objective.id(), self.objective.version())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> AutonomyObjectiveRecord {
		AutonomyObjectiveRecord {
			project_id: self.project_id.clone(),
			objective: self.objective.clone(),
			state: self.state,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomySignalKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal_id: String,
}
impl AutonomySignalKey {
	pub(in crate::state) fn new(project_id: &str, signal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), signal_id: signal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomySignalRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal: AutonomySignal,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomySignalRuntimeRecord {
	pub(in crate::state) fn key(&self) -> AutonomySignalKey {
		AutonomySignalKey::new(&self.project_id, self.signal.id())
	}

	pub(in crate::state) fn as_public(&self) -> AutonomySignalRecord {
		AutonomySignalRecord {
			project_id: self.project_id.clone(),
			signal: self.signal.clone(),
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomyProposalKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal_id: String,
}
impl AutonomyProposalKey {
	pub(in crate::state) fn new(project_id: &str, proposal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), proposal_id: proposal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomyProposalRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal: AutonomyProposal,
	pub(in crate::state) state: AutonomyProposalState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomyProposalRuntimeRecord {
	pub(in crate::state) fn key(&self) -> AutonomyProposalKey {
		AutonomyProposalKey::new(&self.project_id, self.proposal.id())
	}

	pub(in crate::state) fn as_public(&self) -> AutonomyProposalRecord {
		AutonomyProposalRecord {
			project_id: self.project_id.clone(),
			proposal: self.proposal.clone(),
			state: self.state,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct DecisionContractKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) contract_id: String,
}
impl DecisionContractKey {
	pub(in crate::state) fn new(project_id: &str, contract_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), contract_id: contract_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct DecisionContractRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) contract: DecisionContract,
	pub(in crate::state) status: DecisionContractStatus,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl DecisionContractRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> DecisionContractKey {
		DecisionContractKey::new(&self.project_id, self.contract.contract_id())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> DecisionContractRecord {
		DecisionContractRecord {
			project_id: self.project_id.clone(),
			source_issue_id: self.source_issue_id.clone(),
			contract: self.contract.clone(),
			status: self.status,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct LinearExecutionEventRuntimeRecord {
	pub(in crate::state) record: LinearExecutionEventRecord,
	pub(in crate::state) event_unix: Option<i64>,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) record_id: i64,
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload: Value,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}
impl PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> PrivateExecutionEvent {
		PrivateExecutionEvent {
			record_id: self.record_id,
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			event_type: self.event_type.clone(),
			payload: self.payload.clone(),
			recorded_at: self.recorded_at.clone(),
			recorded_at_unix: self.recorded_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct LoopGuardrailKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) reason: String,
}
impl LoopGuardrailKey {
	pub(in crate::state) fn new(project_id: &str, issue_id: &str, reason: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			reason: reason.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct LoopGuardrailRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) reason: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) consecutive_count: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl LoopGuardrailRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> LoopGuardrailCheckpoint {
		LoopGuardrailCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			reason: self.reason.clone(),
			fingerprint: self.fingerprint.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			consecutive_count: self.consecutive_count,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::state) enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ExecutionProgramKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
}
impl ExecutionProgramKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), program_id: program_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ExecutionProgramRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) program: ExecutionProgram,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ExecutionProgramRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> ExecutionProgramKey {
		ExecutionProgramKey::new(&self.project_id, self.program.program_id())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> crate::state::ExecutionProgramRecord {
		crate::state::ExecutionProgramRecord {
			project_id: self.project_id.clone(),
			program: self.program.clone(),
			source_contract_id: self.source_contract_id.clone(),
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ProgramIntakePlanKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) plan_id: String,
}
impl ProgramIntakePlanKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str, plan_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			plan_id: plan_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ProgramIssueMappingKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) node_id: String,
}
impl ProgramIssueMappingKey {
	pub(in crate::state) fn new(project_id: &str, program_id: &str, node_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			node_id: node_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ReviewLifecycleKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
}
impl ReviewLifecycleKey {
	pub(in crate::state) fn new(project_id: &str, issue_id: &str, branch_name: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct ReviewPolicyKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) phase: String,
}
impl ReviewPolicyKey {
	pub(in crate::state) fn new(
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			phase: phase.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct EvidenceArtifactKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) artifact_kind: String,
	pub(in crate::state) key_hash: String,
}
impl EvidenceArtifactKey {
	pub(in crate::state) fn new(
		project_id: &str,
		issue_id: &str,
		artifact_kind: &str,
		key_hash: &str,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			artifact_kind: artifact_kind.to_owned(),
			key_hash: key_hash.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ReviewLifecycleRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) pr_url: String,
	pub(in crate::state) target_base_ref_name: Option<String>,
	pub(in crate::state) pr_head_ref_name: String,
	pub(in crate::state) pr_head_oid: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) phase: String,
	pub(in crate::state) request_comment_database_id: Option<i64>,
	pub(in crate::state) request_created_at_unix_epoch: Option<i64>,
	pub(in crate::state) request_description_thumbs_up_count: Option<usize>,
	pub(in crate::state) request_retry_count: i64,
	pub(in crate::state) external_round_count: i64,
	pub(in crate::state) auto_merge_enabled_at_unix_epoch: Option<i64>,
	pub(in crate::state) landing_state: String,
	pub(in crate::state) closeout_state: String,
	pub(in crate::state) repair_attempt_count: i64,
	pub(in crate::state) evidence_json: String,
	pub(in crate::state) next_action: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ReviewLifecycleRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> ReviewLifecycleRecord {
		ReviewLifecycleRecord {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			pr_url: self.pr_url.clone(),
			target_base_ref_name: self.target_base_ref_name.clone(),
			pr_head_ref_name: self.pr_head_ref_name.clone(),
			pr_head_oid: self.pr_head_oid.clone(),
			head_sha: self.head_sha.clone(),
			phase: self.phase.clone(),
			request_comment_database_id: self.request_comment_database_id,
			request_created_at_unix_epoch: self.request_created_at_unix_epoch,
			request_description_thumbs_up_count: self.request_description_thumbs_up_count,
			request_retry_count: self.request_retry_count,
			external_round_count: self.external_round_count,
			auto_merge_enabled_at_unix_epoch: self.auto_merge_enabled_at_unix_epoch,
			landing_state: self.landing_state.clone(),
			closeout_state: self.closeout_state.clone(),
			repair_attempt_count: self.repair_attempt_count,
			evidence_json: self.evidence_json.clone(),
			next_action: self.next_action.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}

	pub(in crate::state) fn matches_handoff_identity(&self, handoff: &ReviewHandoffMarker) -> bool {
		self.run_id == handoff.run_id()
			&& self.attempt_number == handoff.attempt_number()
			&& self.branch_name == handoff.branch_name()
			&& self.pr_url == handoff.pr_url()
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ReviewPolicyRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) phase: String,
	pub(in crate::state) status: String,
	pub(in crate::state) head_sha: String,
	pub(in crate::state) nonclean_rounds: i64,
	pub(in crate::state) details_json: String,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl ReviewPolicyRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> ReviewPolicyCheckpoint {
		ReviewPolicyCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			phase: self.phase.clone(),
			status: self.status.clone(),
			head_sha: self.head_sha.clone(),
			nonclean_rounds: self.nonclean_rounds,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct EvidenceArtifactRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) artifact_kind: String,
	pub(in crate::state) key_hash: String,
	pub(in crate::state) phase: String,
	pub(in crate::state) status: String,
	pub(in crate::state) head_sha: Option<String>,
	pub(in crate::state) key_json: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) source_run_id: String,
	pub(in crate::state) source_attempt_number: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl EvidenceArtifactRuntimeRecord {
	pub(in crate::state) fn as_review_policy_checkpoint(&self) -> Result<ReviewPolicyCheckpoint> {
		let payload = serde_json::from_str::<Value>(&self.payload_json).map_err(|error| {
			eyre::eyre!(
				"Invalid review checkpoint artifact payload for issue `{}` phase `{}` head `{:?}`: {error}",
				self.issue_id,
				self.phase,
				self.head_sha
			)
		})?;
		let nonclean_rounds =
			payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or_default();
		let details_json =
			payload.get("details_json").and_then(Value::as_str).unwrap_or("{}").to_owned();

		Ok(ReviewPolicyCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.source_run_id.clone(),
			attempt_number: self.source_attempt_number,
			phase: self.phase.clone(),
			status: self.status.clone(),
			head_sha: self.head_sha.clone().unwrap_or_default(),
			nonclean_rounds,
			details_json,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		})
	}
}

pub(in crate::state) struct DecisionContractRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) contract_id: String,
	pub(in crate::state) source_issue_id: Option<String>,
	pub(in crate::state) status: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyObjectiveRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomySignalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) kind: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) freshness: String,
	pub(in crate::state) evidence_class: String,
	pub(in crate::state) confidence: String,
	pub(in crate::state) privacy: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyProposalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) source_family: String,
	pub(in crate::state) intended_surface: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct ExecutionProgramRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) program_id: String,
	pub(in crate::state) source_contract_id: Option<String>,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct WorktreeMappingRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) branch_name: String,
	pub(in crate::state) worktree_path: PathBuf,
	pub(in crate::state) provenance_source: String,
	pub(in crate::state) created_at_unix: Option<i64>,
	pub(in crate::state) updated_at_unix: Option<i64>,
}
impl WorktreeMappingRecord {
	pub(in crate::state) fn as_public(&self) -> WorktreeMapping {
		WorktreeMapping {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			worktree_path: self.worktree_path.clone(),
			provenance: state::worktree_provenance(
				self.provenance_source.clone(),
				self.created_at_unix,
				self.updated_at_unix,
			),
		}
	}
}
