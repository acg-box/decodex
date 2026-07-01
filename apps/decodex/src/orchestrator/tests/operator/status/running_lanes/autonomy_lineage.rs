use crate::orchestrator::tests::FakeTracker;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	config::ServiceConfig,
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::{self, OperatorAutonomyLineageStatus, OperatorStatusSnapshot},
	program_intake::{self, GoalIntakeRunRequest},
	state::{ReviewHandoffMarker, StateStore},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

const SERVICE_ID: &str = "pubfi";
const AUTONOMY_RUN_ID: &str = "run-autonomy";
const OBJECTIVE_ID: &str = "quality-autonomy";

struct SeededAutonomyLineage {
	accepted_proposal_id: String,
	decision_contract_id: String,
	generated_issue_identifier: String,
}

struct ReplayEvidenceSeed<'a> {
	proposal_id: &'a str,
	decision_contract_id: &'a str,
	run_id: &'a str,
	kind: &'a str,
	source_ref: &'a str,
	summary: &'a str,
	pr_head_ref: Option<&'a str>,
	pr_head_oid: Option<&'a str>,
}

#[test]
fn operator_status_surfaces_autonomy_lineage_without_raw_payloads() {
	let (_temp_dir, config, workflow) = super::super::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = super::super::sample_issue("Todo", &[]);
	let seeded = seed_autonomy_lineage(&state_store, &config, &workflow, &issue);
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assert_autonomy_readback(&snapshot, &seeded);
}

#[test]
fn autonomy_lineage_does_not_use_unlinked_review_lifecycle_as_pr_evidence() {
	let (_temp_dir, config, workflow) = super::super::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = super::super::sample_issue("Todo", &[]);
	let seeded =
		seed_autonomy_lineage_without_execution_evidence(&state_store, &config, &workflow, &issue);
	let (generated_issue_id, generated_issue_identifier) =
		generated_issue_link(&state_store, &seeded.decision_contract_id);
	let stale_review_marker = ReviewHandoffMarker::new(
		"stale-review-run",
		1,
		"y/decodex-stale-review",
		"https://github.com/hack-ink/decodex/pull/stale",
		"main",
		"y/decodex-stale-review",
		"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
	);

	state_store
		.upsert_review_handoff_marker(SERVICE_ID, &generated_issue_id, &stale_review_marker)
		.expect("stale review marker should persist");

	for (kind, source_ref, summary) in [
		(
			"validation",
			"validation:cargo-make-check:passed",
			"Repo validation gate passed before review handoff.",
		),
		(
			"post_land",
			"post_land:decodex-land:merge-install-restart-audit",
			"Post-land evidence was recorded after normal lifecycle authority.",
		),
	] {
		record_replay_evidence_event(
			&state_store,
			&generated_issue_id,
			ReplayEvidenceSeed {
				proposal_id: &seeded.accepted_proposal_id,
				decision_contract_id: &seeded.decision_contract_id,
				run_id: "run-dogfood-review",
				kind,
				source_ref,
				summary,
				pr_head_ref: None,
				pr_head_oid: None,
			},
		);
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let lineage = autonomy_lineage_for_seed(&snapshot, &seeded);
	let evidence_kinds = lineage
		.execution_evidence
		.iter()
		.map(|evidence| evidence.kind.as_str())
		.collect::<BTreeSet<_>>();

	assert_eq!(lineage.program_intake[0].intake_kind, "goal_intake");
	assert_eq!(lineage.completeness, "partial");
	assert!(lineage.known_gaps.contains(&String::from("pr_evidence_missing")));
	assert_eq!(evidence_kinds, BTreeSet::from(["post_land", "validation"]));
	assert!(lineage.execution_evidence.iter().all(|evidence| evidence.issue_identifier.as_deref()
		== Some(generated_issue_identifier.as_str())));
	assert!(!lineage.execution_evidence.iter().any(|evidence| {
		evidence.source_refs.iter().any(|source_ref| source_ref.contains("stale"))
	}));
}

#[test]
fn autonomy_lineage_marks_same_pr_stale_head_lifecycle_as_partial() {
	let (_temp_dir, config, workflow) = super::super::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = super::super::sample_issue("Todo", &[]);
	let seeded =
		seed_autonomy_lineage_without_execution_evidence(&state_store, &config, &workflow, &issue);
	let (generated_issue_id, _generated_issue_identifier) =
		generated_issue_link(&state_store, &seeded.decision_contract_id);
	let stale_head_oid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
	let fresh_head_oid = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
	let stale_review_marker = ReviewHandoffMarker::new(
		"run-dogfood-review",
		1,
		"y/decodex-xy-1091",
		"https://github.com/hack-ink/decodex/pull/1091",
		"main",
		"y/decodex-xy-1091",
		stale_head_oid,
	);

	state_store
		.upsert_review_handoff_marker(SERVICE_ID, &generated_issue_id, &stale_review_marker)
		.expect("stale review marker should persist");

	record_replay_evidence_event(
		&state_store,
		&generated_issue_id,
		ReplayEvidenceSeed {
			proposal_id: &seeded.accepted_proposal_id,
			decision_contract_id: &seeded.decision_contract_id,
			run_id: "run-dogfood-review",
			kind: "pr",
			source_ref: "https://github.com/hack-ink/decodex/pull/1091",
			summary: "PR-backed review handoff readback recorded.",
			pr_head_ref: Some("y/decodex-xy-1091"),
			pr_head_oid: Some(fresh_head_oid),
		},
	);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let lineage = autonomy_lineage_for_seed(&snapshot, &seeded);
	let pr_evidence = lineage
		.execution_evidence
		.iter()
		.find(|evidence| evidence.kind == "pr")
		.expect("partial PR replay evidence should render");

	assert_eq!(lineage.completeness, "partial");
	assert_eq!(pr_evidence.completeness, "partial");
	assert!(pr_evidence.known_gaps.contains(&String::from("review_lifecycle_stale_or_mismatched")));
	assert!(lineage.known_gaps.contains(&String::from("review_lifecycle_stale_or_mismatched")));
}

fn seed_autonomy_lineage(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
) -> SeededAutonomyLineage {
	let seeded =
		seed_autonomy_lineage_without_execution_evidence(state_store, config, workflow, issue);
	let generated_issue_identifier = record_dogfood_execution_evidence(
		state_store,
		&seeded.accepted_proposal_id,
		&seeded.decision_contract_id,
	);

	record_sensitive_autonomy_readback_fixture(state_store, issue);

	SeededAutonomyLineage { generated_issue_identifier, ..seeded }
}

fn seed_autonomy_lineage_without_execution_evidence(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
) -> SeededAutonomyLineage {
	seed_autonomy_run(state_store, issue);
	accept_autonomy_objective(state_store);

	let signal_id = record_autonomy_signal(state_store, issue);
	let accepted_proposal_id = record_autonomy_proposals(state_store, issue, &signal_id);
	let decision_contract_id = promote_autonomy_proposal(state_store, &accepted_proposal_id);

	apply_goal_intake(state_store, config, workflow, issue, &decision_contract_id);

	let (_generated_issue_id, generated_issue_identifier) =
		generated_issue_link(state_store, &decision_contract_id);

	SeededAutonomyLineage { accepted_proposal_id, decision_contract_id, generated_issue_identifier }
}

fn seed_autonomy_run(state_store: &StateStore, issue: &TrackerIssue) {
	state_store
		.record_run_attempt(AUTONOMY_RUN_ID, &issue.id, 1, "running")
		.expect("run attempt should record");
	state_store
		.upsert_lease(SERVICE_ID, &issue.id, AUTONOMY_RUN_ID, "In Progress")
		.expect("lease should record");
	state_store
		.append_event(AUTONOMY_RUN_ID, 1, "turn/completed", "{\"turn\":\"1\"}")
		.expect("event should record");
}

fn accept_autonomy_objective(state_store: &StateStore) {
	state_store
		.upsert_autonomy_objective_draft(SERVICE_ID, autonomy_objective_fixture(SERVICE_ID))
		.expect("objective draft should persist");
	state_store
		.accept_autonomy_objective_version(
			SERVICE_ID,
			OBJECTIVE_ID,
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-23T00:00:00Z",
				"linear:XY-1089",
			)
			.expect("acceptance should build"),
		)
		.expect("objective should accept");
}

fn record_autonomy_signal(state_store: &StateStore, issue: &TrackerIssue) -> String {
	let signal = AutonomySignal::runtime_health(AutonomySignalInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Report,
		source_refs: vec![String::from("status:operator:run-autonomy")],
		primary_source_refs: vec![String::from("docs/spec/autonomy-control-plane.md")],
		issue_id: Some(issue.id.clone()),
		run_id: Some(String::from(AUTONOMY_RUN_ID)),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("0123456789abcdef0123456789abcdef01234567")),
		captured_at: String::from("2026-06-23T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Operator status needs autonomy lineage readback."),
		evidence: vec![String::from("Status readback carried source refs.")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: Vec::new(),
		confidence: AutonomySignalConfidence::High,
		privacy: AutonomySignalPrivacy::Public,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:01:00Z"),
	})
	.expect("signal should build");
	let signal_id = signal.id().to_owned();

	state_store.record_autonomy_signal(SERVICE_ID, signal).expect("signal should persist");

	signal_id
}

fn record_autonomy_proposals(
	state_store: &StateStore,
	issue: &TrackerIssue,
	signal_id: &str,
) -> String {
	let signal_ids = vec![signal_id.to_owned()];
	let accepted_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			autonomy_proposal_input("apps/decodex/src/orchestrator/status.rs", &issue.identifier),
			&signal_ids,
		)
		.expect("proposal should compile");
	let accepted_proposal_id = accepted_proposal.id().to_owned();

	state_store
		.record_autonomy_proposal(SERVICE_ID, accepted_proposal)
		.expect("proposal should persist");

	let refused_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			autonomy_proposal_input("site/src/pages/index.astro", &issue.identifier),
			&signal_ids,
		)
		.expect("refused proposal should compile");

	state_store
		.record_autonomy_proposal(SERVICE_ID, refused_proposal)
		.expect("refused proposal should persist");

	accepted_proposal_id
}

fn record_sensitive_autonomy_readback_fixture(state_store: &StateStore, issue: &TrackerIssue) {
	let signal = AutonomySignal::runtime_health(AutonomySignalInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Report,
		source_refs: vec![String::from("status:operator:sensitive-readback")],
		primary_source_refs: vec![String::from("docs/spec/autonomy-control-plane.md")],
		issue_id: Some(issue.id.clone()),
		run_id: Some(String::from(AUTONOMY_RUN_ID)),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("0123456789abcdef0123456789abcdef01234567")),
		captured_at: String::from("2026-06-23T00:04:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Operator status must redact raw autonomy gaps."),
		evidence: vec![String::from("Sensitive fixture stays out of public readback.")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: vec![String::from(
			"Local contradiction references /Users/x/.codex/private-evidence.json",
		)],
		gaps: vec![String::from(
			"Local gap references GITHUB_PAT_Y from the developer environment.",
		)],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Public,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:04:00Z"),
	})
	.expect("sensitive signal fixture should build");
	let signal_id = signal.id().to_owned();

	state_store
		.record_autonomy_signal(SERVICE_ID, signal)
		.expect("sensitive signal should persist");

	let sensitive_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			autonomy_proposal_input("apps/decodex/src/orchestrator/status.rs", &issue.identifier),
			&[signal_id],
		)
		.expect("sensitive proposal should compile");

	state_store
		.record_autonomy_proposal(SERVICE_ID, sensitive_proposal)
		.expect("sensitive proposal should persist");
}

fn promote_autonomy_proposal(state_store: &StateStore, proposal_id: &str) -> String {
	let authority = AutonomyProposalDecisionBridgeAuthority::new(
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		"2026-06-23T00:02:00Z",
		"linear:XY-1089",
		"Accept autonomy lineage proposal.",
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		None,
	)
	.expect("proposal authority should build");
	let decision = state_store
		.accept_autonomy_proposal_as_decision_contract_candidate(SERVICE_ID, proposal_id, authority)
		.expect("decision contract should persist");
	let decision_contract_id = decision.contract_id().to_owned();

	state_store
		.promote_decision_contract(
			SERVICE_ID,
			&decision_contract_id,
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-23T00:03:00Z",
				"linear:XY-1089",
				Some(String::from("Promote accepted autonomy proposal.")),
			)
			.expect("promotion should build"),
		)
		.expect("decision should promote");

	decision_contract_id
}

fn apply_goal_intake(
	state_store: &StateStore,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	issue: &TrackerIssue,
	decision_contract_id: &str,
) {
	let tracker = FakeTracker::new(vec![issue.clone()]);

	program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store,
		tracker: &tracker,
		config,
		workflow,
		contract_id: decision_contract_id,
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("goal intake should apply");
}

fn record_dogfood_execution_evidence(
	state_store: &StateStore,
	proposal_id: &str,
	decision_contract_id: &str,
) -> String {
	let (generated_issue_id, generated_issue_identifier) =
		generated_issue_link(state_store, decision_contract_id);
	let review_marker = ReviewHandoffMarker::new(
		"run-dogfood-review",
		1,
		"y/decodex-xy-1091",
		"https://github.com/hack-ink/decodex/pull/1091",
		"main",
		"y/decodex-xy-1091",
		"0123456789abcdef0123456789abcdef01234567",
	);

	state_store
		.upsert_review_handoff_marker(SERVICE_ID, &generated_issue_id, &review_marker)
		.expect("review handoff marker should persist");

	record_replay_evidence_event(
		state_store,
		&generated_issue_id,
		ReplayEvidenceSeed {
			proposal_id,
			decision_contract_id,
			run_id: "run-dogfood-review",
			kind: "validation",
			source_ref: "validation:cargo-make-check:passed",
			summary: "Local validation summary referenced GITHUB_PAT_Y before clean replay evidence.",
			pr_head_ref: None,
			pr_head_oid: None,
		},
	);

	for (kind, source_ref, summary) in [
		(
			"pr",
			"https://github.com/hack-ink/decodex/pull/1091",
			"PR-backed review handoff readback recorded.",
		),
		(
			"validation",
			"validation:cargo-make-check:passed",
			"Repo validation gate passed before review handoff.",
		),
		(
			"post_land",
			"post_land:decodex-land:merge-install-restart-audit",
			"Post-land evidence was recorded after normal lifecycle authority.",
		),
	] {
		record_replay_evidence_event(
			state_store,
			&generated_issue_id,
			ReplayEvidenceSeed {
				proposal_id,
				decision_contract_id,
				run_id: "run-dogfood-review",
				kind,
				source_ref,
				summary,
				pr_head_ref: (kind == "pr").then_some("y/decodex-xy-1091"),
				pr_head_oid: (kind == "pr").then_some("0123456789abcdef0123456789abcdef01234567"),
			},
		);
	}

	generated_issue_identifier
}

fn generated_issue_link(state_store: &StateStore, decision_contract_id: &str) -> (String, String) {
	let linked = state_store
		.decision_contract(SERVICE_ID, decision_contract_id)
		.expect("decision contract should read")
		.expect("decision contract should exist");

	(
		linked.contract().links().generated_issue_ids()[0].clone(),
		linked.contract().links().generated_issue_identifiers()[0].clone(),
	)
}

fn record_replay_evidence_event(
	state_store: &StateStore,
	generated_issue_id: &str,
	seed: ReplayEvidenceSeed<'_>,
) {
	state_store
		.append_private_execution_event(
			SERVICE_ID,
			generated_issue_id,
			seed.run_id,
			1,
			"autonomy/replay_evidence",
			serde_json::json!({
				"schema": "decodex.autonomy_replay_evidence/1",
				"proposal_id": seed.proposal_id,
				"contract_id": seed.decision_contract_id,
				"kind": seed.kind,
				"source_refs": [seed.source_ref],
				"summary": seed.summary,
				"pr_head_ref": seed.pr_head_ref,
				"pr_head_oid": seed.pr_head_oid,
			}),
		)
		.expect("replay evidence should persist");
}

fn assert_autonomy_readback(snapshot: &OperatorStatusSnapshot, seeded: &SeededAutonomyLineage) {
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let objective =
		loop_status.autonomy_objective.as_ref().expect("objective readback should render");
	let lineage = autonomy_lineage_for_seed(snapshot, seeded);
	let clean_signal = loop_status
		.autonomy_signals
		.iter()
		.find(|signal| signal.source_refs.contains(&String::from("status:operator:run-autonomy")))
		.expect("clean autonomy signal should render");
	let sensitive_signal = loop_status
		.autonomy_signals
		.iter()
		.find(|signal| {
			signal.source_refs.contains(&String::from("status:operator:sensitive-readback"))
		})
		.expect("sensitive autonomy signal should render");
	let refused = loop_status
		.autonomy_proposals
		.iter()
		.find(|proposal| proposal.refusal_reasons.contains(&String::from("disallowed_surface")))
		.expect("refused proposal should render");
	let sensitive_proposal = loop_status
		.autonomy_proposals
		.iter()
		.find(|proposal| proposal.gaps.contains(&String::from("redacted_sensitive_detail")))
		.expect("sensitive proposal should render");
	let report = loop_status.autonomy_report.as_ref().expect("report readback should render");
	let rendered = orchestrator::render_operator_status(snapshot);
	let snapshot_json = serde_json::to_string(snapshot).expect("snapshot should serialize");

	assert_eq!(objective.objective_id, OBJECTIVE_ID);
	assert_eq!(objective.objective_version, 1);
	assert_eq!(clean_signal.freshness, "fresh");
	assert_eq!(sensitive_signal.gaps, ["redacted_sensitive_detail"]);
	assert_eq!(sensitive_signal.contradictions, ["redacted_sensitive_detail"]);
	assert!(sensitive_signal.known_gaps.contains(&String::from("gap_or_contradiction_redacted")));
	assert_eq!(lineage.decision_contracts[0].contract_id, seeded.decision_contract_id);
	assert_eq!(lineage.program_intake[0].intake_kind, "goal_intake");
	assert_eq!(lineage.completeness, "complete");
	assert!(lineage.known_gaps.is_empty());

	assert_dogfood_execution_evidence(lineage, seeded);

	assert_eq!(refused.refusals[0].reason, "disallowed_surface");
	assert_eq!(sensitive_proposal.gaps, ["redacted_sensitive_detail"]);
	assert_eq!(sensitive_proposal.contradictions, ["redacted_sensitive_detail"]);
	assert!(
		report
			.known_gaps
			.iter()
			.any(|gap| gap.contains("redacted") || gap == "proposal_public_fields_redacted")
	);
	assert_eq!(report.authority, "derived_query_view");
	assert!(!report.audit_authority);
	assert_eq!(report.completeness, "partial");
	assert!(rendered.contains("loop_autonomy_signals: runtime_health:quality-autonomy@v1"));
	assert!(rendered.contains("sources=1"));
	assert!(rendered.contains("report=derived_query_view"));
	assert!(!snapshot_json.contains("dry_run_record"));
	assert!(!snapshot_json.contains("/Users/x"));
	assert!(!snapshot_json.contains("GITHUB_PAT_Y"));
	assert!(!rendered.contains("/Users/x"));
	assert!(!rendered.contains("GITHUB_PAT_Y"));
}

fn autonomy_lineage_for_seed<'a>(
	snapshot: &'a OperatorStatusSnapshot,
	seeded: &SeededAutonomyLineage,
) -> &'a OperatorAutonomyLineageStatus {
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");

	loop_status
		.autonomy_lineage
		.iter()
		.find(|lineage| lineage.proposal_id.as_deref() == Some(&seeded.accepted_proposal_id))
		.expect("accepted proposal lineage should render")
}

fn assert_dogfood_execution_evidence(
	lineage: &OperatorAutonomyLineageStatus,
	seeded: &SeededAutonomyLineage,
) {
	let evidence_kinds = lineage
		.execution_evidence
		.iter()
		.map(|evidence| evidence.kind.as_str())
		.collect::<BTreeSet<_>>();
	let source_refs = lineage
		.execution_evidence
		.iter()
		.flat_map(|evidence| evidence.source_refs.iter().map(String::as_str))
		.collect::<BTreeSet<_>>();

	assert_eq!(evidence_kinds, BTreeSet::from(["post_land", "pr", "validation"]));
	assert!(lineage.execution_evidence.iter().all(|evidence| {
		evidence.issue_identifier.as_deref() == Some(&seeded.generated_issue_identifier)
			&& evidence.completeness == "complete"
			&& evidence.known_gaps.is_empty()
	}));
	assert!(source_refs.contains("https://github.com/hack-ink/decodex/pull/1091"));
	assert!(source_refs.contains("validation:cargo-make-check:passed"));
	assert!(source_refs.contains("post_land:decodex-land:merge-install-restart-audit"));
}

fn autonomy_objective_fixture(service_id: &str) -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": service_id,
		"id": OBJECTIVE_ID,
		"version": 1,
		"state": "draft",
		"summary": "Surface autonomy lineage in operator readback.",
		"goals": ["Expose objective, signal, proposal, decision, and intake lineage."],
		"non_goals": ["Do not expose raw private evidence payloads."],
		"metrics": ["Operator can explain autonomy state without SQLite."],
		"allowed_surfaces": ["apps/decodex/src/orchestrator", "docs/spec"],
		"allowed_signal_kinds": ["runtime_health"],
		"validation_gates": ["cargo test -p decodex operator --lib"],
		"review_policy": "independent review before handoff",
		"memory_policy": "runtime records only",
		"report_policy": "public-safe derived query views only"
	}))
	.expect("objective fixture should parse")
}

fn autonomy_proposal_input(
	intended_surface: &str,
	issue_identifier: &str,
) -> AutonomyProposalCompileInput {
	AutonomyProposalCompileInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_family: String::from("operator_status"),
		intended_surface: intended_surface.to_owned(),
		affected_identifiers: vec![issue_identifier.to_owned()],
		summary: String::from("Surface autonomy lineage in operator readback."),
		challenge_requirements: vec![String::from("Verify remote-safe projection.")],
		rejected_alternatives: vec![String::from("Ask operators to inspect SQLite manually.")],
		rollback_path: String::from("Remove the operator readback projection."),
		weakened_validation_or_review: Vec::new(),
		issue_candidates: Vec::new(),
		created_at: String::from("2026-06-23T00:02:00Z"),
	}
}
