use std::slice;

use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
		AutonomyProposalRefusalReason, AutonomyProposalState, tests,
	},
	autonomy_signal::{AutonomySignal, AutonomySignalFreshness},
};

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

#[test]
fn autonomy_proposal_refusal_reasons_are_specific_and_inspectable() {
	let objective = tests::objective_fixture();
	let signal = tests::runtime_signal();
	let missing =
		AutonomyProposal::compile_dry_run(None, slice::from_ref(&signal), tests::compile_input())
			.expect("missing objective proposal should compile as refusal");

	assert_eq!(missing.state(), AutonomyProposalState::NeedsEvidence);
	assert!(missing.has_refusal_reason(AutonomyProposalRefusalReason::MissingObjective));

	let mut stale_input = tests::signal_input();

	stale_input.freshness = AutonomySignalFreshness::Stale;

	let stale_signal =
		AutonomySignal::runtime_health(stale_input).expect("stale signal should validate");
	let stale = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[stale_signal],
		tests::compile_input(),
	)
	.expect("stale evidence proposal should compile as refusal");

	assert_eq!(stale.state(), AutonomyProposalState::NeedsEvidence);
	assert!(stale.has_refusal_reason(AutonomyProposalRefusalReason::StaleEvidence));

	let mut contradiction_input = tests::signal_input();

	contradiction_input.contradictions =
		vec![String::from("Tracker says closed while runtime says active.")];

	let contradictory_signal = AutonomySignal::runtime_health(contradiction_input)
		.expect("contradictory signal should validate");
	let contradictory = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[contradictory_signal],
		tests::compile_input(),
	)
	.expect("contradictory proposal should compile as refusal");

	assert_eq!(contradictory.state(), AutonomyProposalState::NeedsHumanDecision);
	assert!(
		contradictory.has_refusal_reason(AutonomyProposalRefusalReason::UnresolvedContradiction)
	);

	let mut weakened_input = tests::compile_input();

	weakened_input.weakened_validation_or_review =
		vec![String::from("Review evidence is older than the current head.")];

	let weakened = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		weakened_input,
	)
	.expect("weakened validation proposal should compile as refusal");

	assert_eq!(weakened.state(), AutonomyProposalState::NeedsEvidence);
	assert!(weakened.has_refusal_reason(AutonomyProposalRefusalReason::WeakenedValidationReview));

	let mut disallowed_surface_input = tests::compile_input();

	disallowed_surface_input.intended_surface = String::from("scripts/unowned.rs");

	let disallowed_surface = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		disallowed_surface_input,
	)
	.expect("disallowed surface proposal should compile as refusal");

	assert_eq!(disallowed_surface.state(), AutonomyProposalState::Rejected);
	assert!(
		disallowed_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface)
	);

	let mut traversal_surface_input = tests::compile_input();

	traversal_surface_input.intended_surface =
		String::from("apps/decodex/src/../../scripts/unowned.rs");

	let traversal_surface = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		traversal_surface_input,
	)
	.expect("traversal surface proposal should compile as refusal");

	assert_eq!(traversal_surface.state(), AutonomyProposalState::Rejected);
	assert!(traversal_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface));

	let docs_signal = AutonomySignal::docs_skill_drift(tests::signal_input())
		.expect("docs signal should validate");
	let disallowed_kind =
		AutonomyProposal::compile_dry_run(Some(&objective), &[docs_signal], tests::compile_input())
			.expect("disallowed signal proposal should compile as refusal");

	assert_eq!(disallowed_kind.state(), AutonomyProposalState::Rejected);
	assert!(
		disallowed_kind.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSignalKind)
	);
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
