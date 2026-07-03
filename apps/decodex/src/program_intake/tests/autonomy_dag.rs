use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		AutonomyObjectiveState,
	},
	autonomy_proposal::{
		AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalIssueCandidate,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	program_intake::{
		self, GoalIntakeIssueAction, GoalIntakeReport, GoalIntakeRunRequest, tests::test_support,
	},
	state::StateStore,
};

#[test]
fn autonomy_proposal_issue_dag_materializes_through_goal_intake_in_isolated_store() {
	let store = StateStore::open_in_memory().expect("isolated store should open");
	let contract = promoted_autonomy_dag_contract(&store);
	let tracker =
		test_support::FakeTracker::default().with_issues([test_support::issue("XY-2000", "Todo")]);
	let config = test_support::test_config();
	let workflow = test_support::workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: contract.contract_id(),
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("isolated goal intake should materialize the proposal issue DAG");

	assert_autonomy_dag_goal_intake_result(&store, &tracker, &contract, &report);
}

fn promoted_autonomy_dag_contract(store: &StateStore) -> DecisionContract {
	store
		.upsert_autonomy_objective_draft("decodex", autonomy_dag_objective())
		.expect("objective draft should persist in isolated store");

	let objective = store
		.accept_autonomy_objective_version(
			"decodex",
			"isolated-dag-dogfood",
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-30T00:00:00Z",
				"isolated-test",
			)
			.expect("objective acceptance should validate"),
		)
		.expect("objective should accept")
		.objective()
		.clone();
	let proposal_id = persist_autonomy_dag_proposal(store, &objective);
	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			autonomy_dag_bridge_authority(),
		)
		.expect("accepted proposal should persist a latent Decision Contract candidate");
	let mut contract = candidate.contract().clone();

	assert_eq!(
		contract
			.research_provenance()
			.iter()
			.filter(|provenance| provenance.reference() == proposal_id)
			.count(),
		1
	);
	assert_eq!(contract.execution_readiness().proposed_issues().len(), 2);
	assert_eq!(
		contract.execution_readiness().proposed_issues()[1].dependencies(),
		&[String::from("dispatch-provenance")]
	);

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-30T00:03:00Z",
				"isolated-test",
				Some(String::from("Operator accepted isolated DAG materialization test.")),
			)
			.expect("promotion should validate"),
		)
		.expect("contract should promote");
	store
		.upsert_decision_contract("decodex", Some("XY-2000"), contract.clone())
		.expect("promoted contract should persist in isolated store");

	contract
}

fn persist_autonomy_dag_proposal(
	store: &StateStore,
	objective: &AutonomyObjectiveContract,
) -> String {
	let signal = AutonomySignal::runtime_health(autonomy_dag_signal_input())
		.expect("autonomy signal should validate");
	let signal_id = signal.id().to_owned();

	store
		.record_autonomy_signal("decodex", signal)
		.expect("signal should persist in isolated store");

	let proposal = store
		.compile_autonomy_proposal_dry_run(autonomy_dag_proposal_input(), &[signal_id])
		.expect("proposal should compile explicit issue DAG from persisted evidence");
	let proposal_id = proposal.id().to_owned();

	assert_eq!(
		store
			.autonomy_objective("decodex", objective.id(), objective.version())
			.expect("objective should read back")
			.expect("objective should exist")
			.objective()
			.state(),
		AutonomyObjectiveState::Accepted
	);

	store
		.record_autonomy_proposal("decodex", proposal)
		.expect("proposal should persist in isolated store");

	let proposal_record = store
		.record_autonomy_proposal_challenge(
			"decodex",
			&proposal_id,
			AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("isolated-skeptic"),
				summary: String::from("No blocker found for the isolated issue split."),
				objections: Vec::new(),
				evidence_refs: vec![String::from("isolated:test")],
				recorded_at: String::from("2026-06-30T00:02:00Z"),
			},
		)
		.expect("challenge evidence should persist without granting authority");

	assert_eq!(proposal_record.proposal().issue_candidates().len(), 2);
	assert_eq!(
		store
			.autonomy_proposal("decodex", &proposal_id)
			.expect("proposal should read back")
			.expect("proposal should exist")
			.proposal()
			.challenge_evidence()
			.len(),
		1
	);

	proposal_id
}

fn assert_autonomy_dag_goal_intake_result(
	store: &StateStore,
	tracker: &test_support::FakeTracker,
	contract: &DecisionContract,
	report: &GoalIntakeReport,
) {
	assert!(report.applied);
	assert!(report.persisted);
	assert_eq!(tracker.created_issue_count(), 2);
	assert_eq!(tracker.updated_issue_count(), 0);
	assert_eq!(report.issues.len(), 2);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[0].dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(report.issues[1].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[1].dispatch_action, None);
	assert!(
		report.issues[1]
			.reasons
			.iter()
			.any(|reason| reason.contains("has not reached a required terminal state")),
		"dependent node should wait for the first generated issue to complete"
	);

	let programs = store.list_execution_programs("decodex").expect("programs should list");

	assert_eq!(programs.len(), 1);
	assert_eq!(programs[0].program().source_contract_id(), Some(contract.contract_id()));
	assert_eq!(programs[0].program().nodes().len(), 2);

	let program_json = serde_json::to_value(programs[0].program())
		.expect("program should serialize for dependency inspection");

	assert_eq!(
		program_json["nodes"][1]["dependencies"][0]["dependency_id"],
		program_json["nodes"][0]["node_id"]
	);

	let linked_contract = store
		.decision_contract("decodex", contract.contract_id())
		.expect("contract readback should work")
		.expect("linked contract should exist");

	assert_eq!(linked_contract.contract().links().generated_issue_identifiers().len(), 2);

	let intake_plans = store.list_program_intake_plans("decodex").expect("intake plans");

	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some(contract.contract_id()));
}

fn autonomy_dag_objective() -> AutonomyObjectiveContract {
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
		"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
		"allowed_signal_kinds": ["runtime_health"],
		"validation_gates": ["cargo test -p decodex autonomy_proposal --lib"],
		"review_policy": "isolated challenge evidence required before promotion",
		"memory_policy": "source-linked test evidence only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("autonomy objective fixture should parse")
}

fn autonomy_dag_signal_input() -> AutonomySignalInput {
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

fn autonomy_dag_proposal_input() -> AutonomyProposalCompileInput {
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

fn autonomy_dag_issue_candidate(
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

fn autonomy_dag_bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		"2026-06-30T00:02:30Z",
		"isolated-test",
		"Operator accepted the isolated DAG proposal for Decision Contract promotion.",
		"decodex-test-agent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		None,
	)
	.expect("bridge authority should validate")
}
