use std::slice;

use crate::{
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalRefusalReason, AutonomyProposalState, tests,
	},
	autonomy_signal::{AutonomySignal, AutonomySignalFreshness},
};

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

	let docs_signal = AutonomySignal::docs_plugin_drift(tests::signal_input())
		.expect("docs signal should validate");
	let disallowed_kind =
		AutonomyProposal::compile_dry_run(Some(&objective), &[docs_signal], tests::compile_input())
			.expect("disallowed signal proposal should compile as refusal");

	assert_eq!(disallowed_kind.state(), AutonomyProposalState::Rejected);
	assert!(
		disallowed_kind.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSignalKind)
	);
}
