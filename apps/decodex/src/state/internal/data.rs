use std::collections::HashMap;

use crate::{
	prelude::{Result, eyre},
	state::{
		internal::guards::{DispatchSlotConfig, DispatchSlotGuard, IssueClaimGuard},
		models::{
			ConnectorBackoff, IssueLease, ProgramIntakePlanRecord, ProgramIssueMappingRecord,
			ProjectRegistration, ProjectRunStatus,
		},
		runtime_records::{
			AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord, AutonomyProposalKey,
			AutonomyProposalRuntimeRecord, AutonomySignalKey, AutonomySignalRuntimeRecord,
			DecisionContractKey, DecisionContractRuntimeRecord, EvidenceArtifactKey,
			EvidenceArtifactRuntimeRecord, ExecutionProgramKey, ExecutionProgramRuntimeRecord,
			LinearExecutionEventRuntimeRecord, LoopGuardrailKey, LoopGuardrailRuntimeRecord,
			PrivateExecutionEventRuntimeRecord, ProgramIntakePlanKey, ProgramIssueMappingKey,
			ProtocolEventRecord, ProtocolEventSummaryRecord, ReviewLifecycleKey,
			ReviewLifecycleRuntimeRecord, ReviewPolicyKey, ReviewPolicyRuntimeRecord,
			RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord,
			WorktreeMappingRecord,
		},
		runtime_row_parsers,
	},
};

#[derive(Default)]
pub(in crate::state) struct StateData {
	pub(in crate::state) projects: HashMap<String, ProjectRegistration>,
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
	pub(in crate::state) execution_programs:
		HashMap<ExecutionProgramKey, ExecutionProgramRuntimeRecord>,
	pub(in crate::state) program_intake_plans:
		HashMap<ProgramIntakePlanKey, ProgramIntakePlanRecord>,
	pub(in crate::state) program_issue_mappings:
		HashMap<ProgramIssueMappingKey, ProgramIssueMappingRecord>,
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
impl StateData {
	pub(in crate::state) fn replace_durable_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.events = loaded.events;
		self.event_summaries = loaded.event_summaries;
		self.run_activity_summaries = loaded.run_activity_summaries;
		self.worktrees = loaded.worktrees;
		self.linear_execution_events = loaded.linear_execution_events;
		self.private_execution_events = loaded.private_execution_events;
		self.decision_contracts = loaded.decision_contracts;
		self.autonomy_objectives = loaded.autonomy_objectives;
		self.autonomy_signals = loaded.autonomy_signals;
		self.autonomy_proposals = loaded.autonomy_proposals;
		self.execution_programs = loaded.execution_programs;
		self.program_intake_plans = loaded.program_intake_plans;
		self.program_issue_mappings = loaded.program_issue_mappings;
		self.review_lifecycle_records = loaded.review_lifecycle_records;
		self.review_policy_checkpoints = loaded.review_policy_checkpoints;
		self.evidence_artifacts = loaded.evidence_artifacts;
		self.loop_guardrail_checkpoints = loaded.loop_guardrail_checkpoints;
		self.connector_backoffs = loaded.connector_backoffs;
	}

	pub(in crate::state) fn replace_project_run_metadata_state(&mut self, loaded: Self) {
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.run_activity_summaries = loaded.run_activity_summaries;
		self.worktrees = loaded.worktrees;
	}

	pub(in crate::state) fn replace_project_loop_evidence_state(
		&mut self,
		project_id: &str,
		loaded: Self,
	) {
		self.private_execution_events.retain(|record| record.project_id != project_id);
		self.private_execution_events.extend(loaded.private_execution_events);
		self.review_lifecycle_records.retain(|key, _record| key.project_id != project_id);
		self.review_lifecycle_records.extend(loaded.review_lifecycle_records);
		self.review_policy_checkpoints.retain(|key, _record| key.project_id != project_id);
		self.review_policy_checkpoints.extend(loaded.review_policy_checkpoints);
		self.evidence_artifacts.retain(|key, _record| key.project_id != project_id);
		self.evidence_artifacts.extend(loaded.evidence_artifacts);
		self.autonomy_signals.retain(|key, _record| key.project_id != project_id);
		self.autonomy_signals.extend(loaded.autonomy_signals);
		self.autonomy_proposals.retain(|key, _record| key.project_id != project_id);
		self.autonomy_proposals.extend(loaded.autonomy_proposals);
	}

	pub(in crate::state) fn replace_project_registry_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
	}

	pub(in crate::state) fn project_run_status(
		&self,
		project_id: &str,
		attempt: &RunAttemptRecord,
	) -> Option<ProjectRunStatus> {
		let worktree = self.worktrees.get(&attempt.issue_id);
		let run_lease = self
			.leases
			.get(&attempt.issue_id)
			.is_some_and(|lease| lease.project_id == project_id && lease.run_id == attempt.run_id);
		let remembered_project = attempt.project_id.as_deref() == Some(project_id);
		let in_project = remembered_project
			|| worktree.is_some_and(|mapping| mapping.project_id == project_id)
			|| run_lease;

		if !in_project {
			return None;
		}

		let event_summary = self.protocol_event_summary(&attempt.run_id);
		let run_activity_summary = self.run_activity_summaries.get(&attempt.run_id);
		let control_channel = self
			.control_channels
			.get(&attempt.run_id)
			.filter(|channel| {
				channel.project_id == project_id
					&& channel.issue_id == attempt.issue_id
					&& channel.attempt_number == attempt.attempt_number
			})
			.map(RunControlChannelRecord::as_public);
		let mut recovery_evidence = vec![String::from("run_attempt")];

		if run_lease {
			recovery_evidence.push(String::from("active_lease"));
		}
		if control_channel.is_some() {
			recovery_evidence.push(String::from("run_control_channel"));
		}
		if event_summary.event_count > 0 {
			recovery_evidence.push(format!("protocol_events:{}", event_summary.event_count));
		}
		if run_activity_summary.and_then(|summary| summary.child_agent_activity.as_ref()).is_some()
		{
			recovery_evidence.push(String::from("child_agent_activity_summary"));
		}
		if run_activity_summary.and_then(|summary| summary.protocol_activity.as_ref()).is_some() {
			recovery_evidence.push(String::from("protocol_activity_summary"));
		}

		Some(ProjectRunStatus {
			run_id: attempt.run_id.clone(),
			issue_id: attempt.issue_id.clone(),
			attempt_number: attempt.attempt_number,
			status: attempt.status.clone(),
			thread_id: attempt.thread_id.clone(),
			turn_id: attempt.turn_id.clone(),
			updated_at: attempt.updated_at.clone(),
			updated_at_unix: attempt.updated_at_unix,
			branch_name: worktree.map(|mapping| mapping.branch_name.clone()),
			worktree_path: worktree.map(|mapping| mapping.worktree_path.clone()),
			run_lease,
			event_count: event_summary.event_count,
			last_event_type: event_summary.last_event_type,
			last_event_at: event_summary.last_event_at,
			last_event_at_unix: event_summary.last_event_at_unix,
			control_channel,
			child_agent_activity: run_activity_summary
				.and_then(|summary| summary.child_agent_activity.clone()),
			protocol_activity: run_activity_summary
				.and_then(|summary| summary.protocol_activity.clone()),
			recovery_source: String::from("recorded"),
			recovery_evidence,
			recovery_gaps: Vec::new(),
		})
	}

	pub(in crate::state) fn protocol_event_summary(
		&self,
		run_id: &str,
	) -> ProtocolEventSummaryRecord {
		self.event_summaries
			.get(run_id)
			.cloned()
			.or_else(|| {
				self.events
					.get(run_id)
					.map(|events| runtime_row_parsers::protocol_event_summary_from_events(events))
			})
			.unwrap_or_default()
	}

	pub(in crate::state) fn project_id_for_run(
		&self,
		issue_id: &str,
		run_id: &str,
	) -> Option<String> {
		self.leases
			.get(issue_id)
			.filter(|lease| lease.run_id == run_id)
			.map(|lease| lease.project_id.clone())
			.or_else(|| self.worktrees.get(issue_id).map(|mapping| mapping.project_id.clone()))
	}

	pub(in crate::state) fn remember_run_project(
		&mut self,
		project_id: &str,
		issue_id: &str,
		run_id: Option<&str>,
	) {
		for attempt in self
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| run_id.is_none_or(|run_id| attempt.run_id == run_id))
		{
			attempt.project_id = Some(project_id.to_owned());
		}
	}

	pub(in crate::state) fn next_private_execution_event_id(&self) -> Result<i64> {
		self.private_execution_events
			.iter()
			.map(|record| record.record_id)
			.max()
			.unwrap_or(0)
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Private execution event row id overflowed i64."))
	}
}
