use std::slice;

use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalState, tests};

#[test]
fn autonomy_proposal_rejects_invalid_issue_candidate_dag_shape() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let mut duplicate_input = tests::compile_input();

	duplicate_input.issue_candidates = vec![
		tests::issue_candidate("same-key", "runtime", Vec::new()),
		tests::issue_candidate("same-key", "eval", Vec::new()),
	];

	let duplicate = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		duplicate_input,
	)
	.expect_err("duplicate issue candidate keys should fail");

	assert!(duplicate.to_string().contains("duplicated"));

	let mut missing_dependency_input = tests::compile_input();

	missing_dependency_input.issue_candidates = vec![tests::issue_candidate(
		"evaluation-gate",
		"eval",
		vec![String::from("missing-runtime")],
	)];

	let missing_dependency = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		missing_dependency_input,
	)
	.expect_err("missing dependency should fail");

	assert!(missing_dependency.to_string().contains("depends on unknown key"));

	let mut cyclic_input = tests::compile_input();

	cyclic_input.issue_candidates = vec![
		tests::issue_candidate("runtime-work", "runtime", vec![String::from("eval-work")]),
		tests::issue_candidate("eval-work", "eval", vec![String::from("runtime-work")]),
	];

	let cyclic =
		AutonomyProposal::compile_dry_run(Some(&objective), slice::from_ref(&signal), cyclic_input)
			.expect_err("cyclic dependencies should fail");

	assert!(cyclic.to_string().contains("cyclic dependencies"));

	let mut self_dependency_input = tests::compile_input();

	self_dependency_input.issue_candidates =
		vec![tests::issue_candidate("self-cycle", "runtime", vec![String::from("self-cycle")])];

	let self_dependency = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		self_dependency_input,
	)
	.expect_err("self dependency should fail");

	assert!(self_dependency.to_string().contains("cyclic dependencies"));

	let mut invalid_stage_input = tests::compile_input();

	invalid_stage_input.issue_candidates =
		vec![tests::issue_candidate("bad-stage", "implementation", Vec::new())];

	let invalid_stage =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], invalid_stage_input)
			.expect_err("unsupported stage should fail");

	assert!(invalid_stage.to_string().contains("unsupported stage"));
}

#[test]
fn autonomy_proposal_rejects_promoted_state_without_decision_contract_provenance() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let mut proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], tests::compile_input())
			.expect("proposal should compile");

	proposal.state = AutonomyProposalState::AcceptedPromoted;

	assert!(
		proposal
			.validate()
			.expect_err("accepted_promoted should require promotion provenance")
			.to_string()
			.contains("cannot claim accepted_promoted")
	);
}
