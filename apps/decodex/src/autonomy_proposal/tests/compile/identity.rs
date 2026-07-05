use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalChallengeSource, tests,
	},
	autonomy_signal::AutonomySignal,
};

#[test]
fn autonomy_proposal_id_ignores_timestamps_signal_order_warning_order_and_challenges() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let mut second_input = tests::signal_input();

	second_input.source_refs = vec![String::from("status:runtime-health:secondary")];
	second_input.evidence = vec![String::from("secondary readback")];

	let second_signal =
		AutonomySignal::runtime_health(second_input).expect("second signal should validate");
	let mut input_a = tests::compile_input();
	let mut input_b = tests::compile_input();

	input_a.affected_identifiers = vec![String::from("b"), String::from("a")];
	input_a.created_at = String::from("2026-06-22T00:01:00Z");
	input_b.affected_identifiers = vec![String::from("a"), String::from("b")];
	input_b.created_at = String::from("2026-06-22T00:55:00Z");

	let proposal_a = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[signal.clone(), second_signal.clone()],
		input_a,
	)
	.expect("proposal a should compile");
	let mut proposal_b = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[second_signal, signal.clone(), signal],
		input_b,
	)
	.expect("proposal b should compile");
	let original_id = proposal_b.id().to_owned();

	proposal_b
		.record_challenge(AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::InlineSkeptic,
			actor: String::from("inline"),
			summary: String::from("Skeptic noted a possible operator wording gap."),
			objections: Vec::new(),
			evidence_refs: vec![String::from("challenge:inline")],
			recorded_at: String::from("2026-06-22T00:56:00Z"),
		})
		.expect("challenge should record");

	assert_eq!(proposal_a.id(), original_id);
	assert_eq!(proposal_a.fingerprint(), proposal_b.fingerprint());
	assert_eq!(proposal_b.id(), original_id);
}
