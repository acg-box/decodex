mod autonomy_signal_fingerprint_ignores_timestamps_and_counts;
mod autonomy_signal_memory_and_report_sources_require_primary_refs_and_proposal_only;
mod autonomy_signal_persistent_store_round_trips_signal_payload;
mod autonomy_signal_review_feedback_requires_finding_routes_and_current_head_evidence;
mod autonomy_signal_status_readback_exposes_recent_signal_metadata;
mod autonomy_signal_store_round_trips_exact_objective_version;

use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_signal::{
		AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
		AutonomySignalInput, AutonomySignalPrivacy, AutonomySignalReviewEvidence,
		AutonomySignalReviewRoute, AutonomySignalSourceType,
	},
	state::StateStore,
};

fn signal_input() -> AutonomySignalInput {
	AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:XY-1085:runtime-health")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1085")),
		run_id: Some(String::from("xy-1085-attempt-1")),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("3273e45234aa3346e194a7a9e48cd1c58c3e408c")),
		captured_at: String::from("2026-06-22T00:00:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status readback remained internally consistent."),
		evidence: vec![String::from("status readback had no contradictory lane states")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: vec![String::from("No external dashboard readback included.")],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::LocalPrivate,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-22T00:00:05Z"),
	}
}

fn review_evidence() -> AutonomySignalReviewEvidence {
	AutonomySignalReviewEvidence {
		review_phase: String::from("handoff"),
		review_status: String::from("findings"),
		head_sha: String::from("3273e45234aa3346e194a7a9e48cd1c58c3e408c"),
		checkpoint_refs: vec![String::from(
			"review_checkpoint:XY-1085:3273e45234aa3346e194a7a9e48cd1c58c3e408c",
		)],
		finding_routes: vec![AutonomySignalReviewRoute {
			route: String::from("follow_up"),
			finding_source: Some(String::from("accepted_findings")),
			finding_index: Some(0),
			summary: String::from("Follow-up evidence should inform future proposals."),
			evidence_refs: vec![String::from("finding_routes[0]")],
		}],
	}
}

fn objective_fixture(version: u64) -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": version,
		"state": "draft",
		"summary": "Improve Decodex autonomy quality under explicit authority.",
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
		"allowed_signal_kinds": ["runtime_health", "review_feedback_cluster"],
		"validation_gates": ["cargo make check"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("objective fixture should parse")
}

fn accept_objective(store: &StateStore, version: u64) {
	store
		.upsert_autonomy_objective_draft("decodex", objective_fixture(version))
		.expect("draft objective should store");
	store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			version,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				format!("2026-06-22T00:0{version}:00Z"),
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept");
}
