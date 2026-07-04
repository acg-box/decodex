use std::slice;

use crate::{
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal, AutonomyProposalAuthorityActorKind,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalState, tests,
	},
	autonomy_signal::{AutonomySignal, AutonomySignalSourceType},
	loop_contract::{DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind},
	state::StateStore,
};

#[test]
fn autonomy_decision_bridge_accepts_candidate_as_latent_contract_with_lineage_readback() {
	let (store, proposal_id, candidate) = tests::store_challenged_autonomy_candidate();

	tests::assert_autonomy_candidate_shape(&store, &candidate);

	let readback = store
		.decision_contract("decodex", candidate.contract_id())
		.expect("contract readback should work")
		.expect("candidate should persist");

	assert_eq!(readback.contract(), candidate.contract());

	let idempotent = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			tests::bridge_authority(),
		)
		.expect("re-accepting the same latent contract should be idempotent");

	assert_eq!(idempotent.contract(), candidate.contract());

	let missing_promotion_authority = DecisionPromotion::new(
		"",
		DecisionPromotionActorKind::User,
		"2026-06-22T00:04:00Z",
		"conversation",
		Some(String::from("User asked Decodex to promote the accepted candidate.")),
	);

	assert!(missing_promotion_authority.is_err());
	assert_eq!(
		store
			.decision_contract("decodex", candidate.contract_id())
			.expect("contract should read")
			.expect("contract should exist")
			.status(),
		DecisionContractStatus::DraftLatent
	);

	let promoted = store
		.promote_decision_contract(
			"decodex",
			candidate.contract_id(),
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-22T00:05:00Z",
				"conversation",
				Some(String::from("User asked Decodex to promote the accepted candidate.")),
			)
			.expect("promotion authority should validate"),
		)
		.expect("valid promotion should use existing Decision Contract semantics");

	assert_eq!(promoted.status(), DecisionContractStatus::AcceptedPromoted);

	let reaccept_after_promote = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			tests::bridge_authority(),
		)
		.expect_err("accepted promoted contract must not be overwritten by proposal re-accept");

	assert!(reaccept_after_promote.to_string().contains("will not replace"));
	assert_eq!(
		store
			.decision_contract("decodex", candidate.contract_id())
			.expect("contract should read")
			.expect("contract should exist")
			.status(),
		DecisionContractStatus::AcceptedPromoted
	);
}

#[test]
fn autonomy_decision_bridge_reaccept_refuses_generated_link_replacement() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", tests::runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			tests::bridge_authority(),
		)
		.expect("accepted proposal should become a latent Decision Contract");
	let mut linked_contract = candidate.contract().clone();

	linked_contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["node-1"])
		.expect("test contract links should validate");
	store
		.upsert_decision_contract("decodex", candidate.source_issue_id(), linked_contract.clone())
		.expect("linked contract should persist");

	let reaccept_after_links = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			tests::bridge_authority(),
		)
		.expect_err("generated execution links must not be overwritten");

	assert!(reaccept_after_links.to_string().contains("will not replace"));

	let readback = store
		.decision_contract("decodex", candidate.contract_id())
		.expect("contract should read")
		.expect("contract should exist");

	assert_eq!(readback.contract().links().generated_issue_identifiers(), &["XY-G1"]);
	assert_eq!(readback.contract(), &linked_contract);
}

#[test]
fn autonomy_decision_bridge_rejected_and_needs_human_proposals_remain_non_executable() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = tests::store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", tests::runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let mut rejected_input = tests::compile_input();

	rejected_input.intended_surface = String::from("scripts/unowned.rs");

	let rejected = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		rejected_input,
	)
	.expect("rejected proposal should compile");
	let rejected_id = rejected.id().to_owned();

	assert_eq!(rejected.state(), AutonomyProposalState::Rejected);

	store.record_autonomy_proposal("decodex", rejected).expect("rejected proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&rejected_id,
				tests::bridge_authority(),
			)
			.is_err()
	);

	let mut contradiction_input = tests::signal_input();

	contradiction_input.contradictions =
		vec![String::from("Runtime and tracker authority disagree.")];

	let contradiction_signal = store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(contradiction_input).expect("signal should validate"),
		)
		.expect("contradiction signal should store")
		.signal()
		.clone();
	let needs_human = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[contradiction_signal],
		tests::compile_input(),
	)
	.expect("needs-human proposal should compile");
	let needs_human_id = needs_human.id().to_owned();

	assert_eq!(needs_human.state(), AutonomyProposalState::NeedsHumanDecision);

	store
		.record_autonomy_proposal("decodex", needs_human)
		.expect("needs-human proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&needs_human_id,
				tests::bridge_authority(),
			)
			.is_err()
	);
	assert!(
		store
			.list_decision_contracts_for_project("decodex")
			.expect("contracts should list")
			.is_empty()
	);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	assert!(store.list_program_intake_plans("decodex").expect("intake plans").is_empty());
}

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
}
