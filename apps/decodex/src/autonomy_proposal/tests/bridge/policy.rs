use crate::{
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal, AutonomyProposalAuthorityActorKind,
		AutonomyProposalDecisionBridgeAuthority, tests,
	},
	autonomy_signal::{AutonomySignal, AutonomySignalSourceType},
	loop_contract::DecisionContractStatus,
	state::StateStore,
};

fn assert_external_agent_policy_authority_validation() {
	let self_accept_without_policy =
		AutonomyProposalDecisionBridgeAuthority::new(tests::decision_bridge_authority_input(
			"subagent",
			AutonomyProposalAuthorityActorKind::User,
			"agent-output",
			"Agent accepted its own proposal.",
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			None,
		));

	assert!(self_accept_without_policy.is_err());

	let wrong_actor_policy =
		AutonomyProposalDecisionBridgeAuthority::new(tests::decision_bridge_authority_input(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
			"Agent tried to rely on another actor's policy.",
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			Some(tests::accepted_project_policy(
				"other-agent",
				AutonomyProposalAuthorityActorKind::ExternalAgent,
				"runtime-policy",
			)),
		));

	assert!(wrong_actor_policy.is_err());

	let wrong_source_policy =
		AutonomyProposalDecisionBridgeAuthority::new(tests::decision_bridge_authority_input(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
			"Agent tried to rely on a policy for a different source.",
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			Some(tests::accepted_project_policy(
				"subagent",
				AutonomyProposalAuthorityActorKind::ExternalAgent,
				"manual-only",
			)),
		));

	assert!(wrong_source_policy.is_err());

	let missing_acceptance_scope =
		AutonomyProposalDecisionBridgeAuthority::new(tests::decision_bridge_authority_input(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
			"Accepted project policy is missing the required acceptance scope.",
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			Some(tests::accepted_project_policy_fixture(
				"quality-autonomy",
				"subagent",
				AutonomyProposalAuthorityActorKind::ExternalAgent,
				"runtime-policy",
				"other_scope",
			)),
		));

	assert!(missing_acceptance_scope.is_err());
}

fn assert_policy_objective_lineage_required(store: &StateStore, proposal_id: &str) {
	let wrong_objective_policy = tests::accepted_project_policy_fixture(
		"other-objective",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		"runtime-policy",
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE,
	);
	let wrong_objective_authority =
		AutonomyProposalDecisionBridgeAuthority::new(tests::decision_bridge_authority_input(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
			"Accepted project policy references the wrong objective.",
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			Some(wrong_objective_policy),
		))
		.expect("authority validates before proposal lineage is checked");
	let wrong_objective_accept = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			proposal_id,
			wrong_objective_authority,
		)
		.expect_err("policy must match proposal objective lineage");

	assert!(wrong_objective_accept.to_string().contains("does not match proposal"));
}

#[test]
fn autonomy_decision_bridge_external_agent_self_accept_requires_project_policy() {
	assert_external_agent_policy_authority_validation();

	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);
	let mut input = tests::signal_input();

	input.source_type = AutonomySignalSourceType::Agent;

	let signal = store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(input).expect("agent signal should validate"),
		)
		.expect("agent signal should store")
		.signal()
		.clone();
	let proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	assert_policy_objective_lineage_required(&store, &proposal_id);

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			tests::runtime_policy_bridge_authority(),
		)
		.expect("policy-backed external acceptance should bridge to latent contract");

	assert_eq!(candidate.status(), DecisionContractStatus::DraftLatent);
	assert!(candidate.contract().promotion().is_none());
	assert_eq!(
		candidate.contract().accepted_authority().non_goals(),
		&[String::from("Do not bypass accepted review authority.")]
	);
}
