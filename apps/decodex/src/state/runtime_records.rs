use std::path::PathBuf;

use serde_json::Value;

use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::AutonomySignal,
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
	tracker::records::LinearExecutionEventRecord,
};

use super::{
	AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
	ChildAgentActivitySummary, DecisionContractRecord, ExecutionProgramRecord,
	LoopGuardrailCheckpoint, PrivateExecutionEvent, ProtocolActivitySummary, ReviewHandoffMarker,
	ReviewLifecycleRecord, ReviewPolicyCheckpoint, RunAttempt, RunControlChannel, WorktreeMapping,
	worktree_provenance,
};

pub(super) struct TimestampParts {
	pub(super) text: String,
	pub(super) unix: i64,
}

#[derive(Clone, Debug)]
pub(super) struct RunAttemptRecord {
	pub(super) run_id: String,
	pub(super) project_id: Option<String>,
	pub(super) issue_id: String,
	pub(super) attempt_number: i64,
	pub(super) status: String,
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl RunAttemptRecord {
	pub(super) fn as_public(&self) -> RunAttempt {
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
pub(super) struct RunControlChannelRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) transport: String,
	pub(super) channel_path: PathBuf,
	pub(super) status: String,
	pub(super) published_at: String,
	pub(super) published_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl RunControlChannelRecord {
	pub(super) fn as_public(&self) -> RunControlChannel {
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
pub(super) struct ProtocolEventRecord {
	pub(super) sequence_number: i64,
	pub(super) event_type: String,
	pub(super) payload_sha256: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
}
impl ProtocolEventRecord {
	pub(super) fn is_idempotent_replay_of(&self, candidate: &Self) -> bool {
		self.event_type == candidate.event_type && self.payload_sha256 == candidate.payload_sha256
	}
}

#[derive(Clone, Debug, Default)]
pub(super) struct ProtocolEventSummaryRecord {
	pub(super) event_count: i64,
	pub(super) last_sequence_number: Option<i64>,
	pub(super) last_event_type: Option<String>,
	pub(super) last_event_at: Option<String>,
	pub(super) last_event_at_unix: Option<i64>,
}
impl ProtocolEventSummaryRecord {
	pub(super) fn record_event(&mut self, event: &ProtocolEventRecord) {
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
pub(super) struct RunActivitySummaryRecord {
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(super) protocol_activity: Option<ProtocolActivitySummary>,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(super) struct LinearExecutionEventRuntimeRecord {
	pub(super) record: LinearExecutionEventRecord,
	pub(super) event_unix: Option<i64>,
	pub(super) recorded_at: String,
	pub(super) recorded_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(super) struct PrivateExecutionEventRuntimeRecord {
	pub(super) record_id: i64,
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) event_type: String,
	pub(super) payload: Value,
	pub(super) recorded_at: String,
	pub(super) recorded_at_unix: i64,
}
impl PrivateExecutionEventRuntimeRecord {
	pub(super) fn as_public(&self) -> PrivateExecutionEvent {
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
pub(super) struct DecisionContractKey {
	pub(super) project_id: String,
	pub(super) contract_id: String,
}
impl DecisionContractKey {
	pub(super) fn new(project_id: &str, contract_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), contract_id: contract_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(super) struct DecisionContractRuntimeRecord {
	pub(super) project_id: String,
	pub(super) source_issue_id: Option<String>,
	pub(super) contract: DecisionContract,
	pub(super) status: DecisionContractStatus,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl DecisionContractRuntimeRecord {
	#[allow(dead_code)]
	pub(super) fn key(&self) -> DecisionContractKey {
		DecisionContractKey::new(&self.project_id, self.contract.contract_id())
	}

	#[allow(dead_code)]
	pub(super) fn as_public(&self) -> DecisionContractRecord {
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct AutonomyObjectiveKey {
	pub(super) project_id: String,
	pub(super) objective_id: String,
	pub(super) version: u64,
}
impl AutonomyObjectiveKey {
	pub(super) fn new(project_id: &str, objective_id: &str, version: u64) -> Self {
		Self { project_id: project_id.to_owned(), objective_id: objective_id.to_owned(), version }
	}
}

#[derive(Clone, Debug)]
pub(super) struct AutonomyObjectiveRuntimeRecord {
	pub(super) project_id: String,
	pub(super) objective: AutonomyObjectiveContract,
	pub(super) state: AutonomyObjectiveState,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl AutonomyObjectiveRuntimeRecord {
	#[allow(dead_code)]
	pub(super) fn key(&self) -> AutonomyObjectiveKey {
		AutonomyObjectiveKey::new(&self.project_id, self.objective.id(), self.objective.version())
	}

	#[allow(dead_code)]
	pub(super) fn as_public(&self) -> AutonomyObjectiveRecord {
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
pub(super) struct AutonomySignalKey {
	pub(super) project_id: String,
	pub(super) signal_id: String,
}
impl AutonomySignalKey {
	pub(super) fn new(project_id: &str, signal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), signal_id: signal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(super) struct AutonomySignalRuntimeRecord {
	pub(super) project_id: String,
	pub(super) signal: AutonomySignal,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl AutonomySignalRuntimeRecord {
	pub(super) fn key(&self) -> AutonomySignalKey {
		AutonomySignalKey::new(&self.project_id, self.signal.id())
	}

	pub(super) fn as_public(&self) -> AutonomySignalRecord {
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
pub(super) struct AutonomyProposalKey {
	pub(super) project_id: String,
	pub(super) proposal_id: String,
}
impl AutonomyProposalKey {
	pub(super) fn new(project_id: &str, proposal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), proposal_id: proposal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(super) struct AutonomyProposalRuntimeRecord {
	pub(super) project_id: String,
	pub(super) proposal: AutonomyProposal,
	pub(super) state: AutonomyProposalState,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl AutonomyProposalRuntimeRecord {
	pub(super) fn key(&self) -> AutonomyProposalKey {
		AutonomyProposalKey::new(&self.project_id, self.proposal.id())
	}

	pub(super) fn as_public(&self) -> AutonomyProposalRecord {
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
pub(super) struct ExecutionProgramKey {
	pub(super) project_id: String,
	pub(super) program_id: String,
}
impl ExecutionProgramKey {
	pub(super) fn new(project_id: &str, program_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), program_id: program_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(super) struct ExecutionProgramRuntimeRecord {
	pub(super) project_id: String,
	pub(super) source_contract_id: Option<String>,
	pub(super) program: ExecutionProgram,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl ExecutionProgramRuntimeRecord {
	#[allow(dead_code)]
	pub(super) fn key(&self) -> ExecutionProgramKey {
		ExecutionProgramKey::new(&self.project_id, self.program.program_id())
	}

	#[allow(dead_code)]
	pub(super) fn as_public(&self) -> ExecutionProgramRecord {
		ExecutionProgramRecord {
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
pub(super) struct ProgramIntakePlanKey {
	pub(super) project_id: String,
	pub(super) program_id: String,
	pub(super) plan_id: String,
}
impl ProgramIntakePlanKey {
	pub(super) fn new(project_id: &str, program_id: &str, plan_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			plan_id: plan_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ProgramIssueMappingKey {
	pub(super) project_id: String,
	pub(super) program_id: String,
	pub(super) node_id: String,
}
impl ProgramIssueMappingKey {
	pub(super) fn new(project_id: &str, program_id: &str, node_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			node_id: node_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(super) struct WorktreeMappingRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) branch_name: String,
	pub(super) worktree_path: PathBuf,
	pub(super) provenance_source: String,
	pub(super) created_at_unix: Option<i64>,
	pub(super) updated_at_unix: Option<i64>,
}
impl WorktreeMappingRecord {
	pub(super) fn as_public(&self) -> WorktreeMapping {
		WorktreeMapping {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			worktree_path: self.worktree_path.clone(),
			provenance: worktree_provenance(
				self.provenance_source.clone(),
				self.created_at_unix,
				self.updated_at_unix,
			),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ReviewLifecycleKey {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) branch_name: String,
}
impl ReviewLifecycleKey {
	pub(super) fn new(project_id: &str, issue_id: &str, branch_name: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct ReviewPolicyKey {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) phase: String,
}
impl ReviewPolicyKey {
	pub(super) fn new(
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
pub(super) struct EvidenceArtifactKey {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) artifact_kind: String,
	pub(super) key_hash: String,
}
impl EvidenceArtifactKey {
	pub(super) fn new(
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
pub(super) struct ReviewLifecycleRuntimeRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) branch_name: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) pr_url: String,
	pub(super) target_base_ref_name: Option<String>,
	pub(super) pr_head_ref_name: String,
	pub(super) pr_head_oid: String,
	pub(super) head_sha: String,
	pub(super) phase: String,
	pub(super) request_comment_database_id: Option<i64>,
	pub(super) request_created_at_unix_epoch: Option<i64>,
	pub(super) request_description_thumbs_up_count: Option<usize>,
	pub(super) request_retry_count: i64,
	pub(super) external_round_count: i64,
	pub(super) auto_merge_enabled_at_unix_epoch: Option<i64>,
	pub(super) landing_state: String,
	pub(super) closeout_state: String,
	pub(super) repair_attempt_count: i64,
	pub(super) evidence_json: String,
	pub(super) next_action: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl ReviewLifecycleRuntimeRecord {
	pub(super) fn as_public(&self) -> ReviewLifecycleRecord {
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

	pub(super) fn matches_handoff_identity(&self, handoff: &ReviewHandoffMarker) -> bool {
		self.run_id == handoff.run_id()
			&& self.attempt_number == handoff.attempt_number()
			&& self.branch_name == handoff.branch_name()
			&& self.pr_url == handoff.pr_url()
	}
}

#[derive(Clone, Debug)]
pub(super) struct EvidenceArtifactRuntimeRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) artifact_kind: String,
	pub(super) key_hash: String,
	pub(super) phase: String,
	pub(super) status: String,
	pub(super) head_sha: Option<String>,
	pub(super) key_json: String,
	pub(super) payload_json: String,
	pub(super) source_run_id: String,
	pub(super) source_attempt_number: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl EvidenceArtifactRuntimeRecord {
	pub(super) fn as_review_policy_checkpoint(&self) -> Result<ReviewPolicyCheckpoint> {
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

#[derive(Clone, Debug)]
pub(super) struct ReviewPolicyRuntimeRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) phase: String,
	pub(super) status: String,
	pub(super) head_sha: String,
	pub(super) nonclean_rounds: i64,
	pub(super) details_json: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl ReviewPolicyRuntimeRecord {
	pub(super) fn as_public(&self) -> ReviewPolicyCheckpoint {
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct LoopGuardrailKey {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) reason: String,
}
impl LoopGuardrailKey {
	pub(super) fn new(project_id: &str, issue_id: &str, reason: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			reason: reason.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
pub(super) struct LoopGuardrailRuntimeRecord {
	pub(super) project_id: String,
	pub(super) issue_id: String,
	pub(super) reason: String,
	pub(super) fingerprint: String,
	pub(super) run_id: String,
	pub(super) attempt_number: i64,
	pub(super) consecutive_count: i64,
	pub(super) details_json: String,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}
impl LoopGuardrailRuntimeRecord {
	pub(super) fn as_public(&self) -> LoopGuardrailCheckpoint {
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

pub(super) struct DecisionContractRuntimeRowParts {
	pub(super) project_id: String,
	pub(super) contract_id: String,
	pub(super) source_issue_id: Option<String>,
	pub(super) status: String,
	pub(super) payload_json: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

pub(super) struct AutonomyObjectiveRuntimeRowParts {
	pub(super) project_id: String,
	pub(super) objective_id: String,
	pub(super) version: i64,
	pub(super) state: String,
	pub(super) payload_json: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

pub(super) struct AutonomySignalRuntimeRowParts {
	pub(super) project_id: String,
	pub(super) signal_id: String,
	pub(super) objective_id: String,
	pub(super) objective_version: i64,
	pub(super) kind: String,
	pub(super) fingerprint: String,
	pub(super) freshness: String,
	pub(super) evidence_class: String,
	pub(super) confidence: String,
	pub(super) privacy: String,
	pub(super) payload_json: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

pub(super) struct AutonomyProposalRuntimeRowParts {
	pub(super) project_id: String,
	pub(super) proposal_id: String,
	pub(super) objective_id: String,
	pub(super) objective_version: i64,
	pub(super) state: String,
	pub(super) fingerprint: String,
	pub(super) source_family: String,
	pub(super) intended_surface: String,
	pub(super) payload_json: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

pub(super) struct ExecutionProgramRuntimeRowParts {
	pub(super) project_id: String,
	pub(super) program_id: String,
	pub(super) source_contract_id: Option<String>,
	pub(super) payload_json: String,
	pub(super) created_at: String,
	pub(super) created_at_unix: i64,
	pub(super) updated_at: String,
	pub(super) updated_at_unix: i64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}
