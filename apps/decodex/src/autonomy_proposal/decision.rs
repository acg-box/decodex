use std::collections::BTreeSet;

use serde_json::Value;

use crate::autonomy_proposal::{
	AutonomyProposal, AutonomyProposalChallengeEvidence, AutonomyProposalDecisionBridgeAuthority,
	validation,
};

pub(super) fn autonomy_decision_research_provenance(
	proposal: &AutonomyProposal,
	authority: &AutonomyProposalDecisionBridgeAuthority,
) -> Vec<Value> {
	let mut provenance = vec![
		serde_json::json!({
			"kind": "autonomy_proposal",
			"reference": proposal.id.clone(),
			"summary": proposal.summary.clone(),
		}),
		serde_json::json!({
			"kind": "autonomy_objective",
			"reference": format!(
				"{}:{}@{}",
				proposal.project_id, proposal.objective_id, proposal.objective_version
			),
			"summary": proposal.objective_lineage.objective_summary.as_deref().unwrap_or(
				"Accepted autonomy objective version."
			),
		}),
		serde_json::json!({
			"kind": "proposal_acceptance",
			"reference": authority.acceptance_source.clone(),
			"summary": format!(
				"Accepted by {} ({}) at {}.",
				authority.accepted_by,
				authority.accepted_by_kind.as_str(),
				authority.accepted_at
			),
		}),
	];

	if let Some(policy) = &authority.accepted_project_policy {
		provenance.push(serde_json::json!({
			"kind": "project_policy",
			"reference": policy.authority_ref.clone(),
			"summary": format!(
				"Accepted project policy {}@{} authorized `{}` ({}) for autonomy proposal acceptance.",
				policy.accepted_policy_id,
				policy.accepted_policy_version,
				policy.authorized_actor,
				policy.authorized_actor_kind.as_str()
			)
		}));
	}

	provenance
}

pub(super) fn autonomy_decision_research_evidence(proposal: &AutonomyProposal) -> Vec<Value> {
	let mut evidence = proposal
		.source_signals
		.iter()
		.map(|signal| {
			let mut support = vec![
				format!("freshness={}", signal.freshness),
				format!("evidence_class={}", signal.evidence_class),
				format!("confidence={}", signal.confidence),
			];

			if !signal.gaps.is_empty() {
				support.push(format!("gaps={}", signal.gaps.join("; ")));
			}
			if !signal.contradictions.is_empty() {
				support.push(format!("contradictions={}", signal.contradictions.join("; ")));
			}

			serde_json::json!({
				"kind": format!("autonomy_signal:{}", signal.kind),
				"claim": format!("Autonomy signal `{}` contributed to accepted proposal `{}`.", signal.signal_id, proposal.id),
				"support": support.join("; "),
				"source_ref": signal.signal_id.clone(),
			})
		})
		.collect::<Vec<_>>();

	if !proposal.gaps.is_empty() {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_gap",
			"claim": "Accepted proposal retained evidence gaps for downstream review.",
			"support": proposal.gaps.join("; "),
			"source_ref": proposal.id.clone(),
		}));
	}
	if !proposal.contradictions.is_empty() {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_contradiction",
			"claim": "Accepted proposal retained contradictions for downstream authority checks.",
			"support": proposal.contradictions.join("; "),
			"source_ref": proposal.id.clone(),
		}));
	}

	for challenge in &proposal.challenge_evidence {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_challenge",
			"claim": challenge.summary.clone(),
			"support": challenge_support(challenge),
			"source_ref": format!("challenge:{}", challenge.actor),
		}));
	}

	evidence
}

pub(super) fn challenge_support(challenge: &AutonomyProposalChallengeEvidence) -> String {
	if !challenge.objections.is_empty() {
		challenge.objections.join("; ")
	} else if !challenge.evidence_refs.is_empty() {
		format!("evidence_refs={}", challenge.evidence_refs.join("; "))
	} else {
		String::from("Challenge recorded no objections.")
	}
}

pub(super) fn autonomy_decision_research_options(proposal: &AutonomyProposal) -> Vec<Value> {
	let mut options = vec![serde_json::json!({
		"option": proposal.summary.clone(),
		"tradeoffs": option_tradeoffs(proposal),
		"decision": "accepted_as_latent_decision_contract",
		"rejected_reason": null,
	})];

	options.extend(proposal.rejected_alternatives.iter().map(|alternative| {
		serde_json::json!({
			"option": alternative.clone(),
			"tradeoffs": [],
			"decision": null,
			"rejected_reason": "Rejected before autonomy proposal acceptance.",
		})
	}));

	options
}

pub(super) fn option_tradeoffs(proposal: &AutonomyProposal) -> Vec<String> {
	let mut tradeoffs = Vec::new();

	tradeoffs.push(format!("Rollback path: {}", proposal.rollback_path));
	tradeoffs.extend(
		proposal.review_requirements.iter().map(|item| format!("Review requirement: {item}")),
	);
	tradeoffs
		.extend(proposal.validation_gates.iter().map(|item| format!("Validation gate: {item}")));

	tradeoffs
}

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

pub(super) fn proposal_issue_candidate(proposal: &AutonomyProposal) -> Value {
	serde_json::json!({
		"key": format!("autonomy-{}", stable_slug(&proposal.source_family, 48)),
		"title": proposal.summary.clone(),
		"objective": proposal.summary.clone(),
		"stage": proposal_issue_stage(&proposal.intended_surface),
		"dependencies": [],
		"conflict_domains": proposal_conflict_domains(proposal),
		"acceptance": proposal_objectives(proposal),
		"validation": proposal_validation_expectations(proposal),
		"risk": proposal_risk_notes(proposal),
		"queue_intent": "ready_to_queue",
	})
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

pub(super) fn proposal_issue_stage(intended_surface: &str) -> &'static str {
	if intended_surface.starts_with("docs/spec/") {
		"spec"
	} else if intended_surface.starts_with("docs/") {
		"design"
	} else if intended_surface.starts_with("apps/decodex/src/") {
		"runtime"
	} else {
		"design"
	}
}

pub(super) fn proposal_source_issue_identifier(affected_identifiers: &[String]) -> Option<String> {
	affected_identifiers
		.iter()
		.find(|identifier| looks_like_tracker_issue_identifier(identifier))
		.cloned()
}

pub(super) fn looks_like_tracker_issue_identifier(value: &str) -> bool {
	let Some((prefix, number)) = value.split_once('-') else {
		return false;
	};

	!prefix.is_empty()
		&& prefix.bytes().all(|byte| byte.is_ascii_uppercase())
		&& !number.is_empty()
		&& number.bytes().all(|byte| byte.is_ascii_digit())
}

pub(super) fn stable_slug(value: &str, max_len: usize) -> String {
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
