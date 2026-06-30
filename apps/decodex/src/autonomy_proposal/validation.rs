#[allow(clippy::wildcard_imports)]
use super::*;

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

				if !surface_allowed(&input.intended_surface, objective.allowed_surfaces()) {
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

pub(super) fn surface_allowed(intended_surface: &str, allowed_surfaces: &[String]) -> bool {
	let Some(intended_surface) = normalize_repo_relative_path(intended_surface) else {
		return false;
	};

	allowed_surfaces.iter().any(|surface| {
		normalize_repo_relative_path(surface).is_some_and(|surface| {
			intended_surface == surface
				|| intended_surface
					.strip_prefix(&surface)
					.is_some_and(|suffix| suffix.starts_with('/'))
		})
	})
}

pub(super) fn normalize_repo_relative_path(value: &str) -> Option<String> {
	let path = Path::new(value);

	if path.is_absolute() {
		return None;
	}

	let mut parts = Vec::new();

	for component in path.components() {
		let Component::Normal(part) = component else {
			return None;
		};

		parts.push(part.to_str()?);
	}

	if parts.is_empty() {
		return None;
	}

	Some(parts.join("/"))
}

pub(super) fn autonomy_proposal_schema() -> String {
	AUTONOMY_PROPOSAL_SCHEMA.to_owned()
}

pub(super) const fn autonomy_proposal_record_version() -> u16 {
	AUTONOMY_PROPOSAL_RECORD_VERSION
}

pub(super) fn autonomy_proposal_id(fingerprint: &str) -> String {
	format!("autonomy_proposal:{fingerprint}")
}

pub(super) fn autonomy_proposal_fingerprint(proposal: &AutonomyProposal) -> Result<String> {
	let material = serde_json::json!({
		"project_id": proposal.project_id,
		"objective_id": proposal.objective_id,
		"objective_version": proposal.objective_version,
		"source_signal_ids": proposal.source_signal_ids,
		"affected_identifiers": proposal.affected_identifiers,
		"source_family": proposal.source_family,
		"intended_surface": proposal.intended_surface,
	});
	let payload = serde_json::to_vec(&material)?;
	let digest = Sha256::digest(payload);
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(hash)
}

pub(super) fn validate_compile_input(input: &AutonomyProposalCompileInput) -> Result<()> {
	validate_required("autonomy proposal input.project_id", &input.project_id)?;
	validate_required("autonomy proposal input.objective_id", &input.objective_id)?;
	validate_required("autonomy proposal input.source_family", &input.source_family)?;
	validate_required("autonomy proposal input.intended_surface", &input.intended_surface)?;
	validate_required("autonomy proposal input.summary", &input.summary)?;
	validate_required("autonomy proposal input.rollback_path", &input.rollback_path)?;
	validate_required("autonomy proposal input.created_at", &input.created_at)?;
	validate_string_list(
		"autonomy proposal input.affected_identifiers",
		&input.affected_identifiers,
	)?;
	validate_string_list(
		"autonomy proposal input.challenge_requirements",
		&input.challenge_requirements,
	)?;
	validate_string_list(
		"autonomy proposal input.rejected_alternatives",
		&input.rejected_alternatives,
	)?;
	validate_string_list(
		"autonomy proposal input.weakened_validation_or_review",
		&input.weakened_validation_or_review,
	)?;

	if input.objective_version == 0 {
		eyre::bail!("Autonomy proposal input objective_version must be greater than zero.");
	}

	Ok(())
}

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_optional_required(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_sorted_unique(name: &str, values: &[String]) -> Result<()> {
	validate_string_list(name, values)?;

	let mut seen = BTreeSet::new();
	let mut previous = None;

	for value in values {
		if previous.is_some_and(|previous| previous > value.as_str()) {
			eyre::bail!("{name} must be sorted.");
		}
		if !seen.insert(value.as_str()) {
			eyre::bail!("{name} must not contain duplicates.");
		}

		previous = Some(value.as_str());
	}

	Ok(())
}

pub(super) fn unique_sorted_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
	values
		.into_iter()
		.map(|value| value.trim().to_owned())
		.filter(|value| !value.is_empty())
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect()
}
