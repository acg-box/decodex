use std::collections::HashMap;

use serde_json::Value;

use crate::{
	autonomy_objective::AutonomyObjectiveState,
	prelude::{Result, eyre},
	tracker::records::{self, LinearExecutionEventRecord},
};

use super::{
	super::runtime_records::{
		LinearExecutionEventRuntimeRecord, PrivateExecutionEventRuntimeRecord,
		RunActivitySummaryRecord,
	},
	AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
	ChildAgentActivitySummary, DecisionContractRecord, PrivateExecutionEvent,
	ProgramIntakePlanRecord, ProtocolActivitySummary, ReviewLifecycleRecord,
	ReviewPolicyCheckpoint, StateStore, compare_linear_execution_event_runtime_records,
	compare_private_execution_event_runtime_records, parse_linear_execution_event_unix,
	timestamp_parts, validate_private_execution_event_inputs,
};

/// Project-scoped loop evidence cached for one operator status render.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectLoopEvidenceSnapshot {
	private_events: HashMap<(String, String, i64), Vec<PrivateExecutionEvent>>,
	review_lifecycle_records: HashMap<(String, String), ReviewLifecycleRecord>,
	review_checkpoints: HashMap<(String, String, i64, String), ReviewPolicyCheckpoint>,
	decision_contracts: Vec<DecisionContractRecord>,
	autonomy_objectives: Vec<AutonomyObjectiveRecord>,
	autonomy_signals: Vec<AutonomySignalRecord>,
	autonomy_proposals: Vec<AutonomyProposalRecord>,
	program_intake_plans: Vec<ProgramIntakePlanRecord>,
}
impl ProjectLoopEvidenceSnapshot {
	pub(super) fn insert_private_event(&mut self, event: PrivateExecutionEvent) {
		self.private_events
			.entry((event.issue_id().to_owned(), event.run_id().to_owned(), event.attempt_number()))
			.or_default()
			.push(event);
	}

	pub(super) fn insert_review_lifecycle_record(&mut self, record: ReviewLifecycleRecord) {
		self.review_lifecycle_records
			.insert((record.issue_id().to_owned(), record.branch_name().to_owned()), record);
	}

	pub(super) fn insert_review_checkpoint(&mut self, checkpoint: ReviewPolicyCheckpoint) {
		self.review_checkpoints.insert(
			(
				checkpoint.issue_id().to_owned(),
				checkpoint.run_id().to_owned(),
				checkpoint.attempt_number(),
				checkpoint.phase().to_owned(),
			),
			checkpoint,
		);
	}

	pub(super) fn insert_decision_contract(&mut self, contract: DecisionContractRecord) {
		self.decision_contracts.push(contract);
	}

	pub(super) fn insert_autonomy_objective(&mut self, objective: AutonomyObjectiveRecord) {
		self.autonomy_objectives.push(objective);
	}

	pub(super) fn insert_autonomy_signal(&mut self, signal: AutonomySignalRecord) {
		self.autonomy_signals.push(signal);
	}

	pub(super) fn insert_autonomy_proposal(&mut self, proposal: AutonomyProposalRecord) {
		self.autonomy_proposals.push(proposal);
	}

	pub(super) fn insert_program_intake_plan(&mut self, plan: ProgramIntakePlanRecord) {
		self.program_intake_plans.push(plan);
	}

	pub(super) fn sort_private_events(&mut self) {
		for events in self.private_events.values_mut() {
			events.sort_by(|left, right| {
				left.recorded_at_unix()
					.cmp(&right.recorded_at_unix())
					.then_with(|| left.record_id().cmp(&right.record_id()))
			});
		}
	}

	pub(super) fn sort_decision_contracts(&mut self) {
		self.decision_contracts.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.contract_id().cmp(right.contract_id()))
		});
	}

	pub(super) fn sort_autonomy_objectives(&mut self) {
		self.autonomy_objectives.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.objective_id().cmp(right.objective_id()))
				.then_with(|| left.version().cmp(&right.version()))
		});
	}

	pub(super) fn sort_autonomy_signals(&mut self) {
		self.autonomy_signals.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.signal_id().cmp(right.signal_id()))
		});
	}

	pub(super) fn sort_autonomy_proposals(&mut self) {
		self.autonomy_proposals.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.proposal_id().cmp(right.proposal_id()))
		});
	}

	pub(super) fn sort_program_intake_plans(&mut self) {
		self.program_intake_plans.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.program_id().cmp(right.program_id()))
				.then_with(|| left.plan_id().cmp(right.plan_id()))
		});
	}

	pub(crate) fn private_events(
		&self,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> &[PrivateExecutionEvent] {
		self.private_events
			.get(&(issue_id.to_owned(), run_id.to_owned(), attempt_number))
			.map(Vec::as_slice)
			.unwrap_or(&[])
	}

	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn review_lifecycle_record(
		&self,
		issue_id: &str,
		branch_name: &str,
	) -> Option<&ReviewLifecycleRecord> {
		self.review_lifecycle_records.get(&(issue_id.to_owned(), branch_name.to_owned()))
	}

	pub(crate) fn review_lifecycle_records_for_issue(
		&self,
		issue_id: &str,
	) -> Vec<&ReviewLifecycleRecord> {
		let mut records = self
			.review_lifecycle_records
			.iter()
			.filter(|((record_issue_id, _), _)| record_issue_id == issue_id)
			.map(|(_, record)| record)
			.collect::<Vec<_>>();

		records.sort_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.branch_name().cmp(right.branch_name()))
		});

		records
	}

	pub(crate) fn private_events_for_issue(&self, issue_id: &str) -> Vec<&PrivateExecutionEvent> {
		let mut events = self
			.private_events
			.iter()
			.filter(|((event_issue_id, _, _), _)| event_issue_id == issue_id)
			.flat_map(|(_, events)| events.iter())
			.collect::<Vec<_>>();

		events.sort_by(|left, right| {
			left.recorded_at_unix()
				.cmp(&right.recorded_at_unix())
				.then_with(|| left.record_id().cmp(&right.record_id()))
		});

		events
	}

	pub(crate) fn review_policy_checkpoint(
		&self,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Option<&ReviewPolicyCheckpoint> {
		self.review_checkpoints.get(&(
			issue_id.to_owned(),
			run_id.to_owned(),
			attempt_number,
			phase.to_owned(),
		))
	}

	pub(crate) fn recent_autonomy_signals(&self, limit: usize) -> Vec<&AutonomySignalRecord> {
		self.autonomy_signals.iter().take(limit).collect()
	}

	pub(crate) fn recent_autonomy_proposals(&self, limit: usize) -> Vec<&AutonomyProposalRecord> {
		self.autonomy_proposals.iter().take(limit).collect()
	}

	pub(crate) fn autonomy_objective(
		&self,
		objective_id: &str,
		objective_version: u64,
	) -> Option<&AutonomyObjectiveRecord> {
		self.autonomy_objectives.iter().find(|record| {
			record.objective_id() == objective_id && record.version() == objective_version
		})
	}

	pub(crate) fn accepted_autonomy_objectives(&self) -> Vec<&AutonomyObjectiveRecord> {
		self.autonomy_objectives
			.iter()
			.filter(|record| record.state() == AutonomyObjectiveState::Accepted)
			.collect()
	}

	pub(crate) fn decision_contracts_for_autonomy_proposal(
		&self,
		proposal_id: &str,
	) -> Vec<&DecisionContractRecord> {
		self.decision_contracts
			.iter()
			.filter(|record| {
				record.contract().research_provenance().iter().any(|provenance| {
					provenance.kind() == "autonomy_proposal"
						&& provenance.reference() == proposal_id
				})
			})
			.collect()
	}

	pub(crate) fn program_intake_plans_for_contract(
		&self,
		contract_id: &str,
	) -> Vec<&ProgramIntakePlanRecord> {
		self.program_intake_plans
			.iter()
			.filter(|record| record.source_contract_id() == Some(contract_id))
			.collect()
	}
}

impl StateStore {
	pub(crate) fn record_run_activity_summary(
		&self,
		run_id: &str,
		attempt_number: i64,
		child_agent_activity: Option<&ChildAgentActivitySummary>,
		protocol_activity: Option<&ProtocolActivitySummary>,
	) -> Result<()> {
		let now = timestamp_parts();
		let summary = RunActivitySummaryRecord {
			run_id: run_id.to_owned(),
			attempt_number,
			child_agent_activity: child_agent_activity
				.cloned()
				.map(ChildAgentActivitySummary::sealed_durable),
			protocol_activity: protocol_activity.cloned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};
		let mut state = self.lock_without_refresh()?;

		state.run_activity_summaries.insert(run_id.to_owned(), summary.clone());

		self.upsert_run_activity_summary_locked(&summary)
	}

	/// Persist a locally known Linear execution event in the runtime store.
	pub(crate) fn record_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> Result<bool> {
		records::validate_linear_execution_event_record(record)
			.map_err(|error| eyre::eyre!(error))?;

		let now = timestamp_parts();
		let idempotency_key = record.idempotency_key.clone();
		let mut state = self.lock_without_refresh()?;

		if state.linear_execution_events.contains_key(&idempotency_key) {
			return Ok(false);
		}

		let runtime_record = LinearExecutionEventRuntimeRecord {
			record: record.clone(),
			event_unix: parse_linear_execution_event_unix(record),
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};
		let is_new = self.insert_linear_execution_event_if_absent_locked(&runtime_record)?;

		if is_new {
			state.linear_execution_events.insert(idempotency_key, runtime_record);
		}

		Ok(is_new)
	}

	pub(crate) fn forget_linear_execution_event(&self, idempotency_key: &str) -> Result<()> {
		let mut state = self.lock_without_refresh()?;

		state.linear_execution_events.remove(idempotency_key);

		self.delete_linear_execution_event_locked(idempotency_key)
	}

	/// List locally cached Linear execution events for one issue lane.
	pub(crate) fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRecord>> {
		let mut records = match self.list_persisted_linear_execution_events(service_id, issue_id)? {
			Some(records) => records,
			None => {
				let state = self.lock_without_refresh()?;

				state
					.linear_execution_events
					.values()
					.filter(|record| {
						record.record.service_id == service_id && record.record.issue_id == issue_id
					})
					.cloned()
					.collect::<Vec<_>>()
			},
		};

		records.sort_by(compare_linear_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.record).collect())
	}

	/// Append one private execution event to the local runtime evidence ledger.
	pub fn append_private_execution_event(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		event_type: &str,
		payload: Value,
	) -> Result<PrivateExecutionEvent> {
		validate_private_execution_event_inputs(
			project_id,
			issue_id,
			run_id,
			attempt_number,
			event_type,
		)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let mut record = PrivateExecutionEventRuntimeRecord {
			record_id: 0,
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			event_type: event_type.to_owned(),
			payload,
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};

		record.record_id = match self.insert_private_execution_event_locked(&record)? {
			Some(record_id) => record_id,
			None => state.next_private_execution_event_id()?,
		};

		state.private_execution_events.push(record.clone());

		Ok(record.as_public())
	}

	/// List private execution events for one project/issue/run/attempt tuple.
	pub fn list_private_execution_events(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.issue_id == issue_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List private execution events for one project/issue tuple.
	pub(crate) fn list_private_execution_events_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| record.project_id == project_id && record.issue_id == issue_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List private execution events for one project/run/attempt tuple.
	pub fn list_private_execution_events_for_run_attempt(
		&self,
		project_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Build one project-scoped loop evidence snapshot for operator status rendering.
	pub(crate) fn project_loop_evidence_snapshot(
		&self,
		project_id: &str,
	) -> Result<ProjectLoopEvidenceSnapshot> {
		let mut state = self.lock_without_refresh()?;
		let mut snapshot = ProjectLoopEvidenceSnapshot::default();

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		for record in
			state.private_execution_events.iter().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_private_event(record.as_public());
		}
		for record in
			state.review_lifecycle_records.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_lifecycle_record(record.as_public());
		}
		for record in state
			.review_policy_checkpoints
			.values()
			.filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_checkpoint(record.as_public());
		}
		for record in
			state.decision_contracts.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_decision_contract(record.as_public());
		}
		for record in
			state.autonomy_objectives.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_objective(record.as_public());
		}
		for record in
			state.autonomy_signals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_signal(record.as_public());
		}
		for record in
			state.autonomy_proposals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_proposal(record.as_public());
		}
		for record in
			state.program_intake_plans.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_program_intake_plan(record.clone());
		}

		snapshot.sort_private_events();
		snapshot.sort_decision_contracts();
		snapshot.sort_autonomy_objectives();
		snapshot.sort_autonomy_signals();
		snapshot.sort_autonomy_proposals();
		snapshot.sort_program_intake_plans();

		Ok(snapshot)
	}

	/// Read the latest recorded activity timestamp for one run as a Unix epoch.
	pub fn last_run_activity_unix_epoch(&self, run_id: &str) -> Result<Option<i64>> {
		let state = self.lock()?;
		let last_activity = state.run_attempts.get(run_id).map(|attempt| attempt.updated_at_unix);
		let last_event = state.protocol_event_summary(run_id).last_event_at_unix;

		Ok(match (last_activity, last_event) {
			(Some(run_activity), Some(event_activity)) => Some(run_activity.max(event_activity)),
			(Some(run_activity), None) => Some(run_activity),
			(None, Some(event_activity)) => Some(event_activity),
			(None, None) => None,
		})
	}
}
