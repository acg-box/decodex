use crate::autonomy_proposal::{
	AutonomyProposal, AutonomyProposalDecisionBridgeAuthority, validation,
};

pub(super) fn proposal_objectives(proposal: &AutonomyProposal) -> Vec<String> {
	if proposal.goals.is_empty() { vec![proposal.summary.clone()] } else { proposal.goals.clone() }
}

pub(super) fn proposal_constraints(proposal: &AutonomyProposal) -> Vec<String> {
	let mut constraints = proposal
		.allowed_surfaces
		.iter()
		.map(|surface| format!("Allowed surface: {surface}"))
		.collect::<Vec<_>>();

	constraints.extend(
		proposal
			.review_requirements
			.iter()
			.map(|requirement| format!("Review requirement: {requirement}")),
	);
	constraints.extend(
		proposal
			.challenge_requirements
			.iter()
			.map(|requirement| format!("Challenge requirement: {requirement}")),
	);

	for challenge in &proposal.challenge_evidence {
		constraints.extend(
			challenge
				.objections
				.iter()
				.map(|objection| format!("Challenge promotion constraint: {objection}")),
		);
	}

	constraints.push(String::from(
		"Accepted autonomy proposal must remain latent until Decision Contract promotion.",
	));

	constraints
}

pub(super) fn proposal_assumptions(
	proposal: &AutonomyProposal,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Vec<String> {
	let mut assumptions =
		proposal.metrics.iter().map(|metric| format!("Metric: {metric}")).collect::<Vec<_>>();

	assumptions.push(format!(
		"Proposal actor `{}` ({}) is distinct from acceptance authority `{}` ({}) unless resolved accepted project policy is recorded.",
		authority.proposal_actor,
		authority.proposal_actor_kind.as_str(),
		authority.accepted_by,
		authority.accepted_by_kind.as_str()
	));

	assumptions
}

pub(super) fn proposal_objections(proposal: &AutonomyProposal) -> Vec<String> {
	let mut objections = proposal.contradictions.clone();

	objections.extend(proposal.gaps.iter().map(|gap| format!("Evidence gap: {gap}")));

	for challenge in &proposal.challenge_evidence {
		objections.extend(challenge.objections.iter().cloned());
	}

	validation::unique_sorted_strings(objections)
}

pub(super) fn proposal_stop_conditions(proposal: &AutonomyProposal) -> Vec<String> {
	let mut stop_conditions = vec![
		String::from(
			"Stop before Program Intake unless the Decision Contract is promoted with explicit authority.",
		),
		format!("Rollback path: {}", proposal.rollback_path),
	];

	stop_conditions.extend(
		proposal
			.challenge_requirements
			.iter()
			.map(|requirement| format!("Challenge requirement: {requirement}")),
	);

	stop_conditions
}
