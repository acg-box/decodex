use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalReviewEvidence, AutonomySignalReviewRoute, AutonomySignalSourceType,
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
		"validation_gates": ["cargo make check-docs"],
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

#[test]
fn autonomy_signal_fingerprint_ignores_timestamps_and_counts() {
	let signal =
		AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate");
	let mut input = signal_input();

	input.captured_at = String::from("2026-06-22T00:05:00Z");
	input.created_at = String::from("2026-06-22T00:05:05Z");

	input.observed_counts.insert(String::from("validation_retry_count"), 7);

	let changed = AutonomySignal::runtime_health(input)
		.expect("runtime signal with volatile fields should validate");

	assert_eq!(signal.fingerprint(), changed.fingerprint());
	assert_eq!(signal.id(), changed.id());
}

#[test]
fn autonomy_signal_review_feedback_requires_finding_routes_and_current_head_evidence() {
	let mut input = signal_input();

	input.source_type = AutonomySignalSourceType::Review;

	assert!(AutonomySignal::review_feedback_cluster(input.clone()).is_err());

	input.review_evidence = Some(review_evidence());

	let signal = AutonomySignal::review_feedback_cluster(input)
		.expect("review signal should require normalized route evidence");

	assert_eq!(signal.review_evidence().expect("review evidence").finding_routes.len(), 1);
	assert_eq!(signal.head_sha(), Some("3273e45234aa3346e194a7a9e48cd1c58c3e408c"));
}

#[test]
fn autonomy_signal_memory_and_report_sources_require_primary_refs_and_proposal_only() {
	for source_type in [AutonomySignalSourceType::Memory, AutonomySignalSourceType::Report] {
		let mut input = signal_input();

		input.source_type = source_type;
		input.source_refs = vec![String::from("memory:summary:older-context")];
		input.primary_source_refs = Vec::new();
		input.proposal_only = false;

		assert!(AutonomySignal::docs_skill_drift(input.clone()).is_err());

		input.primary_source_refs = vec![String::from("docs/spec/runtime.md")];
		input.proposal_only = true;

		AutonomySignal::docs_skill_drift(input)
			.expect("memory/report signals with primary refs remain proposal-only");
	}
}

#[test]
fn autonomy_signal_store_round_trips_exact_objective_version() {
	let store = StateStore::open_in_memory().expect("store should open");

	accept_objective(&store, 1);

	let signal_v1 =
		AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate");
	let stored_v1 =
		store.record_autonomy_signal("decodex", signal_v1.clone()).expect("signal should store");

	assert_eq!(stored_v1.signal().objective_version(), 1);
	assert_eq!(stored_v1.signal().freshness(), AutonomySignalFreshness::Fresh);
	assert_eq!(stored_v1.signal().gaps(), ["No external dashboard readback included."]);
	assert_eq!(stored_v1.signal().privacy(), AutonomySignalPrivacy::LocalPrivate);

	accept_objective(&store, 2);

	let mut input_v2 = signal_input();

	input_v2.objective_version = 2;
	input_v2.source_refs = vec![String::from("status:XY-1085:runtime-health:v2")];

	let signal_v2 =
		AutonomySignal::runtime_health(input_v2).expect("runtime signal should validate");

	store.record_autonomy_signal("decodex", signal_v2).expect("v2 signal should store");

	let v1_signals = store
		.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 1)
		.expect("v1 signals should list");
	let v2_signals = store
		.list_autonomy_signals_for_objective("decodex", "quality-autonomy", 2)
		.expect("v2 signals should list");

	assert_eq!(v1_signals.len(), 1);
	assert_eq!(v1_signals[0].signal().id(), signal_v1.id());
	assert_eq!(v2_signals.len(), 1);
	assert_ne!(v1_signals[0].signal().id(), v2_signals[0].signal().id());
}

#[test]
fn autonomy_signal_persistent_store_round_trips_signal_payload() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let signal = {
		let store = StateStore::open(&db_path).expect("store should open");

		accept_objective(&store, 1);

		let signal =
			AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate");

		store.record_autonomy_signal("decodex", signal.clone()).expect("signal should store");

		signal
	};
	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let stored = reopened
		.autonomy_signal("decodex", signal.id())
		.expect("signal read should succeed")
		.expect("signal should exist");

	assert_eq!(stored.signal(), &signal);
	assert_eq!(stored.signal().source_refs(), ["status:XY-1085:runtime-health"]);
	assert!(stored.signal().primary_source_refs().is_empty());
}

#[test]
fn autonomy_signal_status_readback_exposes_recent_signal_metadata() {
	let store = StateStore::open_in_memory().expect("store should open");

	accept_objective(&store, 1);

	store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate"),
		)
		.expect("signal should store");

	let snapshot =
		store.project_loop_evidence_snapshot("decodex").expect("loop evidence should load");
	let recent = snapshot.recent_autonomy_signals(1);
	let signal = recent[0].signal();

	assert_eq!(signal.objective_id(), "quality-autonomy");
	assert_eq!(signal.objective_version(), 1);
	assert_eq!(signal.freshness(), AutonomySignalFreshness::Fresh);
	assert_eq!(signal.confidence(), AutonomySignalConfidence::Medium);
	assert_eq!(signal.privacy(), AutonomySignalPrivacy::LocalPrivate);
	assert_eq!(signal.gaps(), ["No external dashboard readback included."]);
}
