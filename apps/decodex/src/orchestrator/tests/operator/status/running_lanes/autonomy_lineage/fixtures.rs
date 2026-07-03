use std::collections::BTreeMap;

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
	orchestrator::tests::FakeTracker,
	program_intake::{self, GoalIntakeRunRequest},
	state::{ReviewHandoffMarker, StateStore},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(super) const SERVICE_ID: &str = "pubfi";
pub(super) const AUTONOMY_RUN_ID: &str = "run-autonomy";
pub(super) const OBJECTIVE_ID: &str = "quality-autonomy";

pub(super) struct SeededAutonomyLineage {
	pub(super) accepted_proposal_id: String,
	pub(super) decision_contract_id: String,
	pub(super) generated_issue_identifier: String,
}

pub(super) struct ReplayEvidenceSeed<'a> {
	pub(super) proposal_id: &'a str,
	pub(super) decision_contract_id: &'a str,
	pub(super) run_id: &'a str,
	pub(super) kind: &'a str,
	pub(super) source_ref: &'a str,
	pub(super) summary: &'a str,
	pub(super) pr_head_ref: Option<&'a str>,
	pub(super) pr_head_oid: Option<&'a str>,
}

pub(super) fn seed_autonomy_lineage(
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

pub(super) fn seed_autonomy_lineage_without_execution_evidence(
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

pub(super) fn record_dogfood_execution_evidence(
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

pub(super) fn generated_issue_link(
	state_store: &StateStore,
	decision_contract_id: &str,
) -> (String, String) {
	let linked = state_store
		.decision_contract(SERVICE_ID, decision_contract_id)
		.expect("decision contract should read")
		.expect("decision contract should exist");

	(
		linked.contract().links().generated_issue_ids()[0].clone(),
		linked.contract().links().generated_issue_identifiers()[0].clone(),
	)
}

pub(super) fn record_replay_evidence_event(
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
