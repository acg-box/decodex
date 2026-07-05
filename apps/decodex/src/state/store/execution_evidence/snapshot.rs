mod autonomy;

use std::collections::HashMap;

use crate::state::store::{
	AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord, DecisionContractRecord,
	PrivateExecutionEvent, ProgramIntakePlanRecord, ReviewLifecycleRecord, ReviewPolicyCheckpoint,
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
}
