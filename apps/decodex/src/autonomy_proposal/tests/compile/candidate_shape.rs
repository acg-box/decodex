use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalState, tests};

#[test]
fn autonomy_proposal_dry_run_candidate_shows_lineage_signals_gates_and_gaps() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.objective_id(), "quality-autonomy");
	assert_eq!(proposal.objective_version(), 1);
	assert_eq!(proposal.allowed_surfaces(), ["apps/decodex/src", "docs/spec"]);
	assert_eq!(proposal.validation_gates(), ["cargo test -p decodex autonomy_proposal --lib"]);
	assert_eq!(proposal.source_signal_ids().len(), 1);
	assert_eq!(proposal.gaps(), ["No dashboard comparison included."]);
	assert!(proposal.contradictions().is_empty());
	assert!(proposal.refusal_reasons().is_empty());

	let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

	assert_eq!(dry_run_json["dry_run"], true);
	assert_eq!(dry_run_json["non_executable"], true);
	assert_eq!(dry_run_json["objective_lineage"]["objective_id"], "quality-autonomy");
	assert_eq!(dry_run_json["source_signals"][0]["signal_id"], proposal.source_signal_ids()[0]);
	assert_eq!(dry_run_json["allowed_surfaces"][0], "apps/decodex/src");
	assert_eq!(dry_run_json["goals"][0], "Reduce repeated validation and review churn.");
	assert_eq!(
		dry_run_json["metrics"][0],
		"Validation retry count stays below objective tolerance."
	);
	assert_eq!(dry_run_json["non_goals"][0], "Do not bypass Decision Contract authority.");
	assert_eq!(dry_run_json["review_requirements"][0], "independent current-head review required");
	assert_eq!(
		dry_run_json["challenge_requirements"][0],
		"Subagent or inline skeptic objections are evidence only."
	);
	assert_eq!(dry_run_json["rejected_alternatives"][0], "Direct Decision Contract promotion.");
	assert_eq!(dry_run_json["rollback_path"], "Discard the dry-run proposal record.");
	assert_eq!(
		dry_run_json["validation_gates"][0],
		"cargo test -p decodex autonomy_proposal --lib"
	);
	assert!(dry_run_json["refusal_reasons"].as_array().expect("refusals array").is_empty());
}

#[test]
fn autonomy_proposal_can_carry_explicit_dependent_issue_candidates_into_decision_contract() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let mut input = tests::compile_input();

	input.issue_candidates = vec![
		tests::issue_candidate("readback-contract", "runtime", Vec::new()),
		tests::issue_candidate("evaluation-gate", "eval", vec![String::from("readback-contract")]),
	];

	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], input)
		.expect("proposal with explicit issue candidates should compile");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.issue_candidates().len(), 2);

	let contract = proposal
		.to_decision_contract_candidate(tests::bridge_authority())
		.expect("proposal should bridge to latent decision contract");
	let proposed_issues = contract.execution_readiness().proposed_issues();

	assert_eq!(proposed_issues.len(), 2);
	assert_eq!(proposed_issues[0].key(), "readback-contract");
	assert_eq!(proposed_issues[0].stage(), "runtime");
	assert_eq!(proposed_issues[1].key(), "evaluation-gate");
	assert_eq!(proposed_issues[1].stage(), "eval");
	assert_eq!(proposed_issues[1].dependencies(), &[String::from("readback-contract")]);
	assert_eq!(proposed_issues[1].queue_intent(), "ready_to_queue");
}
