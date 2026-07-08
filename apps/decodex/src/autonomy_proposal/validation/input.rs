use std::collections::BTreeSet;

use crate::{
	autonomy_proposal::{
		AutonomyProposalCompileInput, AutonomyProposalIssueCandidate, validation::common,
	},
	prelude::{Result, eyre},
};

pub(super) fn validate_compile_input(input: &AutonomyProposalCompileInput) -> Result<()> {
	common::validate_required("autonomy proposal input.project_id", &input.project_id)?;
	common::validate_required("autonomy proposal input.objective_id", &input.objective_id)?;
	common::validate_required("autonomy proposal input.source_family", &input.source_family)?;
	common::validate_required("autonomy proposal input.intended_surface", &input.intended_surface)?;
	common::validate_required("autonomy proposal input.summary", &input.summary)?;
	common::validate_required("autonomy proposal input.rollback_path", &input.rollback_path)?;
	common::validate_required("autonomy proposal input.created_at", &input.created_at)?;
	common::validate_string_list(
		"autonomy proposal input.affected_identifiers",
		&input.affected_identifiers,
	)?;
	common::validate_string_list(
		"autonomy proposal input.challenge_requirements",
		&input.challenge_requirements,
	)?;
	common::validate_string_list(
		"autonomy proposal input.rejected_alternatives",
		&input.rejected_alternatives,
	)?;
	common::validate_string_list(
		"autonomy proposal input.weakened_validation_or_review",
		&input.weakened_validation_or_review,
	)?;

	validate_issue_candidates(&input.issue_candidates)?;

	if input.objective_version == 0 {
		eyre::bail!("Autonomy proposal input objective_version must be greater than zero.");
	}

	Ok(())
}

pub(super) fn validate_issue_candidates(
	issue_candidates: &[AutonomyProposalIssueCandidate],
) -> Result<()> {
	let mut keys = BTreeSet::new();

	for issue_candidate in issue_candidates {
		issue_candidate.validate()?;

		if !keys.insert(issue_candidate.key.as_str()) {
			eyre::bail!(
				"Autonomy proposal issue candidate key `{}` is duplicated.",
				issue_candidate.key
			);
		}
	}
	for issue_candidate in issue_candidates {
		for dependency in &issue_candidate.dependencies {
			if !keys.contains(dependency.as_str()) {
				eyre::bail!(
					"Autonomy proposal issue candidate `{}` depends on unknown key `{}`.",
					issue_candidate.key,
					dependency
				);
			}
		}
	}

	let mut visited = BTreeSet::new();

	for issue_candidate in issue_candidates {
		let mut visiting = BTreeSet::new();

		validate_issue_candidate_acyclic(
			issue_candidate.key.as_str(),
			issue_candidates,
			&mut visiting,
			&mut visited,
		)?;
	}

	Ok(())
}

pub(super) fn validate_proposed_issue_stage(key: &str, stage: &str) -> Result<()> {
	match stage {
		"design" | "spec" | "schema" | "runtime" | "plugin" | "eval" | "handoff" => Ok(()),
		_ => {
			eyre::bail!(
				"Autonomy proposal issue candidate `{key}` has unsupported stage `{stage}`."
			)
		},
	}
}

pub(super) fn validate_proposed_issue_queue_intent(key: &str, queue_intent: &str) -> Result<()> {
	match queue_intent {
		"not_ready" | "ready_to_queue" | "queued" | "active" | "paused" | "done" | "canceled" =>
			Ok(()),
		_ => eyre::bail!(
			"Autonomy proposal issue candidate `{key}` has unsupported queue_intent `{queue_intent}`."
		),
	}
}

fn validate_issue_candidate_acyclic<'a>(
	key: &'a str,
	issue_candidates: &'a [AutonomyProposalIssueCandidate],
	visiting: &mut BTreeSet<&'a str>,
	visited: &mut BTreeSet<&'a str>,
) -> Result<()> {
	if visited.contains(key) {
		return Ok(());
	}
	if !visiting.insert(key) {
		eyre::bail!("Autonomy proposal issue candidate `{key}` has cyclic dependencies.");
	}

	let Some(issue_candidate) =
		issue_candidates.iter().find(|issue_candidate| issue_candidate.key == key)
	else {
		return Ok(());
	};

	for dependency in &issue_candidate.dependencies {
		validate_issue_candidate_acyclic(dependency.as_str(), issue_candidates, visiting, visited)?;
	}

	visiting.remove(key);
	visited.insert(key);

	Ok(())
}
