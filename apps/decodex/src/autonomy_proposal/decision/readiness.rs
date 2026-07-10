use std::collections::BTreeSet;

use serde_json::Value;

use crate::autonomy_proposal::{AutonomyProposal, decision, validation};

pub(super) fn proposal_validation_expectations(proposal: &AutonomyProposal) -> Vec<String> {
	if proposal.validation_gates.is_empty() {
		vec![String::from("Run the accepted Decision Contract validation gate before promotion.")]
	} else {
		proposal.validation_gates.clone()
	}
}

pub(super) fn proposal_risk_notes(proposal: &AutonomyProposal) -> Vec<String> {
	let mut risk_notes =
		proposal.gaps.iter().map(|gap| format!("Evidence gap: {gap}")).collect::<Vec<_>>();

	risk_notes.extend(
		proposal
			.review_requirements
			.iter()
			.map(|requirement| format!("Review requirement: {requirement}")),
	);
	risk_notes.extend(
		proposal
			.challenge_requirements
			.iter()
			.map(|requirement| format!("Challenge requirement: {requirement}")),
	);

	risk_notes
}

pub(super) fn proposal_issue_candidates(proposal: &AutonomyProposal) -> Vec<Value> {
	if proposal.issue_candidates.is_empty() {
		return vec![proposal_issue_candidate(proposal)];
	}

	proposal
		.issue_candidates
		.iter()
		.map(|candidate| {
			serde_json::json!({
				"key": candidate.key.clone(),
				"title": candidate.title.clone(),
				"objective": candidate.objective.clone(),
				"stage": candidate.stage.clone(),
				"dependencies": candidate.dependencies.clone(),
				"conflict_domains": candidate.conflict_domains.clone(),
				"acceptance": candidate.acceptance.clone(),
				"validation": candidate.validation.clone(),
				"risk": candidate.risk.clone(),
				"queue_intent": candidate.queue_intent.clone(),
			})
		})
		.collect()
}

pub(super) fn proposal_conflict_domains(proposal: &AutonomyProposal) -> Vec<String> {
	let mut domains = BTreeSet::new();

	if let Some(surface) = validation::normalize_repo_relative_path(&proposal.intended_surface) {
		domains.insert(format!("file:{surface}"));
	} else {
		domains.insert(format!("surface:{}", proposal.intended_surface));
	}

	for identifier in &proposal.affected_identifiers {
		domains.insert(format!("identifier:{identifier}"));
	}

	domains.into_iter().collect()
}

pub(super) fn proposal_source_issue_identifier(affected_identifiers: &[String]) -> Option<String> {
	affected_identifiers
		.iter()
		.find(|identifier| looks_like_tracker_issue_identifier(identifier))
		.cloned()
}

fn proposal_issue_candidate(proposal: &AutonomyProposal) -> Value {
	serde_json::json!({
		"key": format!("autonomy-{}", stable_slug(&proposal.source_family, 48)),
		"title": proposal.summary.clone(),
		"objective": proposal.summary.clone(),
		"stage": proposal_issue_stage(&proposal.intended_surface),
		"dependencies": [],
		"conflict_domains": proposal_conflict_domains(proposal),
		"acceptance": decision::proposal_objectives(proposal),
		"validation": proposal_validation_expectations(proposal),
		"risk": proposal_risk_notes(proposal),
		"queue_intent": "ready_to_queue",
	})
}

fn proposal_issue_stage(intended_surface: &str) -> &'static str {
	if intended_surface.starts_with("apps/decodex/src/") { "runtime" } else { "design" }
}

fn looks_like_tracker_issue_identifier(value: &str) -> bool {
	let Some((prefix, number)) = value.split_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& prefix.bytes().all(|byte| byte.is_ascii_uppercase())
		&& !number.is_empty()
		&& number.bytes().all(|byte| byte.is_ascii_digit())
}

fn stable_slug(value: &str, max_len: usize) -> String {
	let mut slug = String::new();
	let mut previous_dash = false;

	for character in value.chars() {
		if character.is_ascii_alphanumeric() {
			slug.push(character.to_ascii_lowercase());

			previous_dash = false;
		} else if !previous_dash && !slug.is_empty() {
			slug.push('-');

			previous_dash = true;
		}
		if slug.len() >= max_len {
			break;
		}
	}

	while slug.ends_with('-') {
		slug.pop();
	}

	if slug.is_empty() { String::from("proposal") } else { slug }
}
