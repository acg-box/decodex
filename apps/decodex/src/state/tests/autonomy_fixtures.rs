use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalCompileInput},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
};

pub(crate) fn autonomy_objective_fixture(version: u64) -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": version,
		"state": "draft",
		"summary": format!("Improve Decodex autonomy quality version {version}."),
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src", "apps/decodex/src/config"],
		"allowed_signal_kinds": ["validation_regression", "review_feedback_cluster"],
		"validation_gates": ["cargo make check"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("autonomy objective fixture should deserialize")
}

pub(crate) fn sample_objective_acceptance() -> AutonomyObjectiveAcceptance {
	AutonomyObjectiveAcceptance::new(
		"operator",
		AutonomyObjectiveActorKind::User,
		"2026-06-22T10:00:00Z",
		"conversation",
	)
	.expect("sample objective acceptance should validate")
}
pub(crate) fn accepted_autonomy_objective_fixture() -> AutonomyObjectiveContract {
	let mut objective = autonomy_objective_fixture(1);

	objective.accept(sample_objective_acceptance()).expect("objective should accept");

	objective
}

pub(crate) fn autonomy_signal_fixture() -> AutonomySignal {
	AutonomySignal::validation_regression(AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:runtime-health")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1086")),
		run_id: Some(String::from("xy-1086-attempt-1")),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("3cd19609c44cb18bff9e7a34a2f4853754afcee0")),
		captured_at: String::from("2026-06-22T00:00:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status readback showed repeated friction."),
		evidence: vec![String::from("status readback retained the repeated friction signal")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: vec![String::from("No dashboard comparison included.")],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-22T00:00:05Z"),
	})
	.expect("runtime signal should validate")
}

pub(crate) fn autonomy_proposal_fixture() -> AutonomyProposal {
	AutonomyProposal::compile_dry_run(
		Some(&accepted_autonomy_objective_fixture()),
		&[autonomy_signal_fixture()],
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
		},
	)
	.expect("autonomy proposal should compile")
}
