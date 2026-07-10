use crate::state::internal::data::StateData;

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
		self.autonomy_runtime_policies = loaded.autonomy_runtime_policies;
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
}
