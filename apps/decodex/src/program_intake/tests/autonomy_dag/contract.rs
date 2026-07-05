use crate::{
	autonomy_objective::{AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind},
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	program_intake::tests::autonomy_dag::{
		fixtures::{self},
		proposal,
	},
	state::StateStore,
};

pub(crate) fn promoted_autonomy_dag_contract(store: &StateStore) -> DecisionContract {
	store
		.upsert_autonomy_objective_draft("decodex", fixtures::autonomy_dag_objective())
		.expect("objective draft should persist in isolated store");

	let objective = store
		.accept_autonomy_objective_version(
			"decodex",
			"isolated-dag-dogfood",
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-30T00:00:00Z",
				"isolated-test",
			)
			.expect("objective acceptance should validate"),
		)
		.expect("objective should accept")
		.objective()
		.clone();
	let proposal_id = proposal::persist_autonomy_dag_proposal(store, &objective);
	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			fixtures::autonomy_dag_bridge_authority(),
		)
		.expect("accepted proposal should persist a latent Decision Contract candidate");
	let mut contract = candidate.contract().clone();

	assert_eq!(
		contract
			.research_provenance()
			.iter()
			.filter(|provenance| provenance.reference() == proposal_id)
			.count(),
		1
	);
	assert_eq!(contract.execution_readiness().proposed_issues().len(), 2);
	assert_eq!(
		contract.execution_readiness().proposed_issues()[1].dependencies(),
		&[String::from("dispatch-provenance")]
	);

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-30T00:03:00Z",
				"isolated-test",
				Some(String::from("Operator accepted isolated DAG materialization test.")),
			)
			.expect("promotion should validate"),
		)
		.expect("contract should promote");
	store
		.upsert_decision_contract("decodex", Some("XY-2000"), contract.clone())
		.expect("promoted contract should persist in isolated store");

	contract
}
