use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{
		AutonomyProposalCompileInput, AutonomyProposalRefusal, AutonomyProposalRefusalReason,
		AutonomyProposalState, validation::paths,
	},
	autonomy_signal::{AutonomySignal, AutonomySignalFreshness},
};

pub(super) fn proposal_refusals(
	objective: Option<&AutonomyObjectiveContract>,
	signals: &[AutonomySignal],
	input: &AutonomyProposalCompileInput,
	contradictions: &[String],
) -> Vec<AutonomyProposalRefusal> {
	let mut refusals = Vec::new();

	match objective {
		Some(objective)
			if objective.project_id() == input.project_id
				&& objective.id() == input.objective_id
				&& objective.version() == input.objective_version
				&& objective.state() == AutonomyObjectiveState::Accepted => {
				for signal in signals {
					if signal.project_id() != input.project_id
						|| signal.objective_id() != input.objective_id
						|| signal.objective_version() != input.objective_version
					{
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::MissingObjective,
							format!(
								"Signal `{}` is not tied to objective `{}` version {}.",
								signal.id(),
								input.objective_id,
								input.objective_version
							),
							vec![signal.id().to_owned()],
						));
					}
					if !objective
						.allowed_signal_kinds()
						.iter()
						.any(|kind| kind == signal.kind().as_str())
					{
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::DisallowedSignalKind,
							format!(
								"Signal `{}` kind `{}` is outside the accepted objective allowed_signal_kinds.",
								signal.id(),
								signal.kind().as_str()
							),
							vec![signal.id().to_owned()],
						));
					}
					if signal.freshness() != AutonomySignalFreshness::Fresh {
						refusals.push(AutonomyProposalRefusal::new(
							AutonomyProposalRefusalReason::StaleEvidence,
							format!(
								"Signal `{}` freshness is `{}` and requires fresh readback before acceptance.",
								signal.id(),
								signal.freshness().as_str()
							),
							vec![signal.id().to_owned()],
						));
					}
				}

				if !paths::surface_allowed(&input.intended_surface, objective.allowed_surfaces()) {
					refusals.push(AutonomyProposalRefusal::new(
						AutonomyProposalRefusalReason::DisallowedSurface,
						format!(
							"Intended surface `{}` is outside the accepted objective allowed_surfaces.",
							input.intended_surface
						),
						vec![input.objective_id.clone()],
					));
				}
			},
		Some(objective) => refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::MissingObjective,
			format!(
				"Objective `{}` version {} exists in state `{}` but is not the accepted exact proposal objective.",
				objective.id(),
				objective.version(),
				objective.state().as_str()
			),
			vec![input.objective_id.clone()],
		)),
		None => refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::MissingObjective,
			format!(
				"Objective `{}` version {} is missing.",
				input.objective_id,
				input.objective_version
			),
			vec![input.objective_id.clone()],
		)),
	}

	for contradiction in contradictions {
		refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::UnresolvedContradiction,
			format!("Contradiction remains unresolved: {contradiction}"),
			vec![input.objective_id.clone()],
		));
	}
	for note in &input.weakened_validation_or_review {
		refusals.push(AutonomyProposalRefusal::new(
			AutonomyProposalRefusalReason::WeakenedValidationReview,
			format!("Validation or review evidence is weakened: {note}"),
			vec![input.objective_id.clone()],
		));
	}

	refusals
}

pub(super) fn derive_proposal_state(
	has_signals: bool,
	refusals: &[AutonomyProposalRefusal],
) -> AutonomyProposalState {
	if refusals.iter().any(|refusal| {
		matches!(
			refusal.reason,
			AutonomyProposalRefusalReason::DisallowedSignalKind
				| AutonomyProposalRefusalReason::DisallowedSurface
		)
	}) {
		return AutonomyProposalState::Rejected;
	}
	if refusals
		.iter()
		.any(|refusal| refusal.reason == AutonomyProposalRefusalReason::UnresolvedContradiction)
	{
		return AutonomyProposalState::NeedsHumanDecision;
	}
	if !refusals.is_empty() {
		return AutonomyProposalState::NeedsEvidence;
	}
	if has_signals {
		AutonomyProposalState::DecisionCandidate
	} else {
		AutonomyProposalState::Draft
	}
}
