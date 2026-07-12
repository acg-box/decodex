mod events;
mod project_runs;
mod replacements;

use std::collections::HashMap;

use crate::{
	lane_authority::{
		IntakeAuthority, LaneAggregate, LaneEffect, LaneId, NoEffectiveDeltaRecovery,
		RepairHandoffAuthority, RepairHandoffState, RoutingQuarantine,
		SupersededCloseoutOperation, SupersessionEdge,
	},
	state::{
		ProgramIntakeAttemptStatus,
		internal::guards::{DispatchSlotConfig, DispatchSlotGuard, IssueClaimGuard},
		models::{
			ConnectorBackoff, IssueLease, ProgramIntakePlanRecord, ProgramIssueMappingRecord,
			ProjectRegistration,
		},
		runtime_records::{
			AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord, AutonomyProposalKey,
			AutonomyProposalRuntimeRecord, AutonomyRuntimePolicyKey,
			AutonomyRuntimePolicyRuntimeRecord, AutonomySignalKey, AutonomySignalRuntimeRecord,
			DecisionContractKey, DecisionContractRuntimeRecord, EvidenceArtifactKey,
			EvidenceArtifactRuntimeRecord, ExecutionProgramKey, ExecutionProgramRuntimeRecord,
			LinearExecutionEventRuntimeRecord, LoopGuardrailKey, LoopGuardrailRuntimeRecord,
			PrivateExecutionEventRuntimeRecord, ProgramIntakePlanKey, ProgramIssueMappingKey,
			ProtocolEventRecord, ProtocolEventSummaryRecord, ReviewLifecycleKey,
			ReviewLifecycleRuntimeRecord, ReviewPolicyKey, ReviewPolicyRuntimeRecord,
			RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord,
			WorktreeMappingRecord,
		},
	},
};

#[derive(Default)]
pub(in crate::state) struct StateData {
	pub(in crate::state) projects: HashMap<String, ProjectRegistration>,
	pub(in crate::state) routing_quarantines: HashMap<String, RoutingQuarantine>,
	pub(in crate::state) lanes: HashMap<LaneId, LaneAggregate>,
	pub(in crate::state) lane_effects: HashMap<String, LaneEffect>,
	pub(in crate::state) no_effective_delta_recoveries: HashMap<String, NoEffectiveDeltaRecovery>,
	pub(in crate::state) repair_handoffs: HashMap<String, RepairHandoffAuthority>,
	pub(in crate::state) repair_handoff_states: HashMap<String, RepairHandoffState>,
	pub(in crate::state) supersession_edges: HashMap<LaneId, SupersessionEdge>,
	pub(in crate::state) superseded_closeout_operations:
		HashMap<String, SupersededCloseoutOperation>,
	pub(in crate::state) leases: HashMap<String, IssueLease>,
	pub(in crate::state) run_attempts: HashMap<String, RunAttemptRecord>,
	pub(in crate::state) control_channels: HashMap<String, RunControlChannelRecord>,
	pub(in crate::state) events: HashMap<String, Vec<ProtocolEventRecord>>,
	pub(in crate::state) event_summaries: HashMap<String, ProtocolEventSummaryRecord>,
	pub(in crate::state) run_activity_summaries: HashMap<String, RunActivitySummaryRecord>,
	pub(in crate::state) worktrees: HashMap<String, WorktreeMappingRecord>,
	pub(in crate::state) linear_execution_events:
		HashMap<String, LinearExecutionEventRuntimeRecord>,
	pub(in crate::state) private_execution_events: Vec<PrivateExecutionEventRuntimeRecord>,
	pub(in crate::state) decision_contracts:
		HashMap<DecisionContractKey, DecisionContractRuntimeRecord>,
	pub(in crate::state) autonomy_objectives:
		HashMap<AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord>,
	pub(in crate::state) autonomy_signals: HashMap<AutonomySignalKey, AutonomySignalRuntimeRecord>,
	pub(in crate::state) autonomy_proposals:
		HashMap<AutonomyProposalKey, AutonomyProposalRuntimeRecord>,
	pub(in crate::state) autonomy_runtime_policies:
		HashMap<AutonomyRuntimePolicyKey, AutonomyRuntimePolicyRuntimeRecord>,
	pub(in crate::state) execution_programs:
		HashMap<ExecutionProgramKey, ExecutionProgramRuntimeRecord>,
	pub(in crate::state) intake_authorities: HashMap<(String, String), IntakeAuthority>,
	pub(in crate::state) program_intake_plans:
		HashMap<ProgramIntakePlanKey, ProgramIntakePlanRecord>,
	pub(in crate::state) program_issue_mappings:
		HashMap<ProgramIssueMappingKey, ProgramIssueMappingRecord>,
	pub(in crate::state) program_intake_attempts:
		HashMap<(String, String), (ProgramIntakeAttemptStatus, String)>,
	pub(in crate::state) review_lifecycle_records:
		HashMap<ReviewLifecycleKey, ReviewLifecycleRuntimeRecord>,
	pub(in crate::state) review_policy_checkpoints:
		HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	pub(in crate::state) evidence_artifacts:
		HashMap<EvidenceArtifactKey, EvidenceArtifactRuntimeRecord>,
	pub(in crate::state) loop_guardrail_checkpoints:
		HashMap<LoopGuardrailKey, LoopGuardrailRuntimeRecord>,
	pub(in crate::state) connector_backoffs: HashMap<(String, String), ConnectorBackoff>,
	pub(in crate::state) dispatch_slot_configs: HashMap<String, DispatchSlotConfig>,
	pub(in crate::state) issue_claim_guards: HashMap<String, IssueClaimGuard>,
	pub(in crate::state) dispatch_slot_guards: HashMap<String, DispatchSlotGuard>,
}
impl StateData {}
