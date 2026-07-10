use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalAuthorityActorKind,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalDecisionBridgeAuthorityInput,
		tests,
	},
	loop_contract::{DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind},
	state::StateStore,
};

#[test]
fn accepts_candidate_contract_with_lineage_readback() {
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

	let conflicting_authority = AutonomyProposalDecisionBridgeAuthority::new(
		AutonomyProposalDecisionBridgeAuthorityInput {
			accepted_by: String::from("other-operator"),
			accepted_by_kind: AutonomyProposalAuthorityActorKind::User,
			accepted_at: String::from("2026-06-22T00:03:00Z"),
			acceptance_source: String::from("conversation"),
			reason: String::from("Conflicting same-id authority."),
			proposal_actor: String::from("subagent"),
			proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
			accepted_project_policy: None,
		},
	)
	.expect("conflicting authority fixture should validate");
	let conflict = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			conflicting_authority,
		)
		.expect_err("same-id latent contract with different authority must be refused");

	assert!(conflict.to_string().contains("will not replace"));

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
