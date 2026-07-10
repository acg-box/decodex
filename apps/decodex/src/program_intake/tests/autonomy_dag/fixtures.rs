use std::collections::BTreeMap;

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalDecisionBridgeAuthorityInput,
		AutonomyProposalIssueCandidate,
	},
	autonomy_signal::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalInput, AutonomySignalPrivacy, AutonomySignalSourceType,
	},
};

pub(crate) fn autonomy_dag_objective() -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "isolated-dag-dogfood",
		"version": 1,
		"state": "draft",
		"summary": "Test Decodex DAG decomposition without touching live service state.",
		"goals": [
			"Prove accepted autonomy proposals can materialize dependent execution work."
		],
		"non_goals": [
			"Do not touch live Linear, GitHub, worktrees, installs, restarts, or plugin sync."
		],
		"metrics": ["Isolated test creates one internal Execution Program with dependent nodes."],
		"allowed_surfaces": ["apps/decodex/src", "apps/decodex/src/config"],
		"allowed_signal_kinds": ["runtime_health"],
		"validation_gates": ["cargo test -p decodex autonomy_proposal --lib"],
		"review_policy": "isolated challenge evidence required before promotion",
		"memory_policy": "source-linked test evidence only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("autonomy objective fixture should parse")
}

pub(crate) fn autonomy_dag_signal_input() -> AutonomySignalInput {
	AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("isolated-dag-dogfood"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("isolated:runtime-readback")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-2000")),
		run_id: None,
		attempt_id: None,
		head_sha: None,
		captured_at: String::from("2026-06-30T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Isolated runtime evidence supports a dependent issue split."),
		evidence: vec![String::from("isolated fake tracker and in-memory store only")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: Vec::new(),
		confidence: AutonomySignalConfidence::High,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-30T00:01:05Z"),
	}
}

pub(crate) fn autonomy_dag_proposal_input() -> AutonomyProposalCompileInput {
	AutonomyProposalCompileInput {
		project_id: String::from("decodex"),
		objective_id: String::from("isolated-dag-dogfood"),
		objective_version: 1,
		source_family: String::from("runtime_health"),
		intended_surface: String::from("apps/decodex/src/orchestrator/program_reconciler.rs"),
		affected_identifiers: vec![
			String::from("XY-2000"),
			String::from("program_dispatch_selected"),
		],
		summary: String::from("Materialize an isolated dependent DAG from autonomy evidence."),
		challenge_requirements: vec![String::from("Record skeptic challenge before promotion.")],
		rejected_alternatives: vec![String::from("Run Program Intake directly from a signal.")],
		rollback_path: String::from("Discard the in-memory proposal and generated fake issues."),
		weakened_validation_or_review: Vec::new(),
		issue_candidates: vec![
			autonomy_dag_issue_candidate("dispatch-provenance", "runtime", Vec::new()),
			autonomy_dag_issue_candidate(
				"daily-evaluation",
				"eval",
				vec![String::from("dispatch-provenance")],
			),
		],
		created_at: String::from("2026-06-30T00:01:30Z"),
	}
}

pub(crate) fn autonomy_dag_issue_candidate(
	key: &str,
	stage: &str,
	dependencies: Vec<String>,
) -> AutonomyProposalIssueCandidate {
	AutonomyProposalIssueCandidate {
		key: key.to_owned(),
		title: format!("Isolated DAG test: {key}"),
		objective: format!("Prove {key} as part of the isolated DAG materialization test."),
		stage: stage.to_owned(),
		dependencies,
		conflict_domains: vec![format!("module:{stage}")],
		acceptance: vec![format!("{key} acceptance evidence is visible in the report.")],
		validation: vec![String::from("cargo test -p decodex program_intake --lib")],
		risk: vec![String::from("Keep the test isolated from live services.")],
		queue_intent: String::from("ready_to_queue"),
	}
}

pub(crate) fn autonomy_dag_bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: String::from("operator"),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::User,
		accepted_at: String::from("2026-06-30T00:02:30Z"),
		acceptance_source: String::from("isolated-test"),
		reason: String::from(
			"Operator accepted the isolated DAG proposal for Decision Contract promotion.",
		),
		proposal_actor: String::from("decodex-test-agent"),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: None,
	})
	.expect("bridge authority should validate")
}
