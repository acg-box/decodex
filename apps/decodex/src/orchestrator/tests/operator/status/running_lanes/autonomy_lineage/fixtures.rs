mod evidence;
mod objective;
mod proposal;
mod runtime;
mod signal;

use crate::{
	config::ServiceConfig,
	orchestrator::tests::operator::status::running_lanes::autonomy_lineage::fixtures::evidence::ExecutionEvidenceSeed,
	state::StateStore, tracker::TrackerIssue, workflow::WorkflowDocument,
};

pub(super) const SERVICE_ID: &str = "pubfi";
pub(super) const AUTONOMY_RUN_ID: &str = "run-autonomy";
pub(super) const OBJECTIVE_ID: &str = "quality-autonomy";

pub(super) struct SeededAutonomyLineage {
	pub(super) accepted_proposal_id: String,
	pub(super) decision_contract_id: String,
	pub(super) generated_issue_identifier: String,
}

pub(super) struct ReplayEvidenceSeed<'a> {
	pub(super) proposal_id: &'a str,
	pub(super) decision_contract_id: &'a str,
	pub(super) run_id: &'a str,
	pub(super) kind: &'a str,
	pub(super) source_ref: &'a str,
	pub(super) summary: &'a str,
	pub(super) pr_head_ref: Option<&'a str>,
	pub(super) pr_head_oid: Option<&'a str>,
}

pub(super) fn seed_autonomy_lineage(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
) -> SeededAutonomyLineage {
	let seeded =
		seed_autonomy_lineage_without_execution_evidence(state_store, config, workflow, issue);
	let generated_issue_identifier = evidence::record_dogfood_execution_evidence(
		state_store,
		ExecutionEvidenceSeed {
			proposal_id: &seeded.accepted_proposal_id,
			decision_contract_id: &seeded.decision_contract_id,
		},
	);

	signal::record_sensitive_autonomy_readback_fixture(state_store, issue);

	SeededAutonomyLineage { generated_issue_identifier, ..seeded }
}

pub(super) fn seed_autonomy_lineage_without_execution_evidence(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
) -> SeededAutonomyLineage {
	runtime::seed_autonomy_run(state_store, issue);
	objective::accept_autonomy_objective(state_store);

	let signal_id = signal::record_autonomy_signal(state_store, issue);
	let accepted_proposal_id = proposal::record_autonomy_proposals(state_store, issue, &signal_id);
	let decision_contract_id =
		proposal::promote_autonomy_proposal(state_store, &accepted_proposal_id);

	runtime::apply_goal_intake(state_store, config, workflow, issue, &decision_contract_id);

	let (_generated_issue_id, generated_issue_identifier) =
		evidence::generated_issue_link(state_store, &decision_contract_id);

	SeededAutonomyLineage { accepted_proposal_id, decision_contract_id, generated_issue_identifier }
}

pub(super) fn generated_issue_link(
	state_store: &StateStore,
	decision_contract_id: &str,
) -> (String, String) {
	evidence::generated_issue_link(state_store, decision_contract_id)
}

pub(super) fn record_replay_evidence_event(
	state_store: &StateStore,
	generated_issue_id: &str,
	seed: ReplayEvidenceSeed<'_>,
) {
	evidence::record_replay_evidence_event(state_store, generated_issue_id, seed);
}
