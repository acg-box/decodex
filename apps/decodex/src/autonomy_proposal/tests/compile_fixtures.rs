use crate::autonomy_proposal::{AutonomyProposalCompileInput, AutonomyProposalIssueCandidate};

pub(crate) fn compile_input() -> AutonomyProposalCompileInput {
	AutonomyProposalCompileInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_family: String::from("runtime_status"),
		intended_surface: String::from("apps/decodex/src/orchestrator/status.rs"),
		affected_identifiers: vec![
			String::from("OperatorLoopStatus"),
			String::from("operator_status"),
		],
		summary: String::from("Compile a bounded proposal from runtime friction evidence."),
		challenge_requirements: vec![String::from(
			"Subagent or inline skeptic objections are evidence only.",
		)],
		rejected_alternatives: vec![String::from("Direct Decision Contract promotion.")],
		rollback_path: String::from("Discard the dry-run proposal record."),
		weakened_validation_or_review: Vec::new(),
		issue_candidates: Vec::new(),
		created_at: String::from("2026-06-22T00:01:00Z"),
	}
}

pub(crate) fn issue_candidate(
	key: &str,
	stage: &str,
	dependencies: Vec<String>,
) -> AutonomyProposalIssueCandidate {
	AutonomyProposalIssueCandidate {
		key: key.to_owned(),
		title: format!("Issue candidate {key}"),
		objective: format!("Complete issue candidate {key}."),
		stage: stage.to_owned(),
		dependencies,
		conflict_domains: vec![format!("issue:{key}")],
		acceptance: vec![format!("{key} acceptance criterion is met.")],
		validation: vec![String::from("cargo test -p decodex autonomy_proposal --lib")],
		risk: vec![String::from("Keep autonomy proposal non-executable until promotion.")],
		queue_intent: String::from("ready_to_queue"),
	}
}
