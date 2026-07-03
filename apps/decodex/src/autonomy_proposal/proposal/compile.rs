use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalCompileInput, AutonomyProposalObjectiveLineage,
		AutonomyProposalSourceSignal, validation,
	},
	autonomy_signal::AutonomySignal,
	prelude::Result,
};

#[allow(dead_code)]
impl AutonomyProposal {
	pub(crate) fn compile_dry_run(
		objective: Option<&AutonomyObjectiveContract>,
		signals: &[AutonomySignal],
		input: AutonomyProposalCompileInput,
	) -> Result<Self> {
		validation::validate_compile_input(&input)?;

		for signal in signals {
			signal.validate()?;
		}

		let objective_lineage = AutonomyProposalObjectiveLineage {
			project_id: input.project_id.clone(),
			objective_id: input.objective_id.clone(),
			objective_version: input.objective_version,
			objective_state: objective.map(|objective| objective.state().as_str().to_owned()),
			objective_summary: objective.map(|objective| objective.summary().to_owned()),
		};
		let mut source_signals =
			signals.iter().map(AutonomyProposalSourceSignal::from_signal).collect::<Vec<_>>();

		source_signals.sort_by(|left, right| left.signal_id.cmp(&right.signal_id));
		source_signals.dedup_by(|left, right| left.signal_id == right.signal_id);

		let source_signal_ids = validation::unique_sorted_strings(
			source_signals.iter().map(|signal| signal.signal_id.clone()),
		);
		let allowed_surfaces =
			objective.map(|objective| objective.allowed_surfaces().to_vec()).unwrap_or_default();
		let validation_gates =
			objective.map(|objective| objective.validation_gates().to_vec()).unwrap_or_default();
		let goals = objective.map(|objective| objective.goals().to_vec()).unwrap_or_default();
		let metrics = objective.map(|objective| objective.metrics().to_vec()).unwrap_or_default();
		let non_goals =
			objective.map(|objective| objective.non_goals().to_vec()).unwrap_or_default();
		let review_requirements = objective
			.map(|objective| vec![objective.review_policy().to_owned()])
			.unwrap_or_default();
		let contradictions = validation::unique_sorted_strings(
			signals.iter().flat_map(|signal| signal.contradictions().to_vec()),
		);
		let gaps = validation::unique_sorted_strings(
			signals.iter().flat_map(|signal| signal.gaps().to_vec()),
		);
		let refusal_reasons =
			validation::proposal_refusals(objective, signals, &input, &contradictions);
		let state =
			validation::derive_proposal_state(!source_signal_ids.is_empty(), &refusal_reasons);
		let affected_identifiers = validation::unique_sorted_strings(input.affected_identifiers);
		let issue_candidates = input.issue_candidates;
		let mut proposal = Self {
			schema: validation::autonomy_proposal_schema(),
			record_version: validation::autonomy_proposal_record_version(),
			id: String::new(),
			fingerprint: String::new(),
			project_id: input.project_id,
			objective_id: input.objective_id,
			objective_version: input.objective_version,
			state,
			source_family: input.source_family,
			intended_surface: input.intended_surface,
			affected_identifiers,
			summary: input.summary,
			objective_lineage,
			source_signal_ids,
			source_signals,
			allowed_surfaces,
			validation_gates,
			goals,
			metrics,
			non_goals,
			review_requirements,
			challenge_requirements: validation::unique_sorted_strings(input.challenge_requirements),
			rejected_alternatives: validation::unique_sorted_strings(input.rejected_alternatives),
			rollback_path: input.rollback_path,
			issue_candidates,
			contradictions,
			gaps,
			refusal_reasons,
			challenge_evidence: Vec::new(),
			dry_run: true,
			non_executable: true,
			created_at: input.created_at,
		};
		let fingerprint = validation::autonomy_proposal_fingerprint(&proposal)?;

		proposal.id = validation::autonomy_proposal_id(&fingerprint);
		proposal.fingerprint = fingerprint;

		proposal.validate()?;

		Ok(proposal)
	}
}
