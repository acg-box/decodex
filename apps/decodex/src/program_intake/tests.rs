use std::{
	cell::RefCell,
	collections::{BTreeMap, HashMap},
	fs,
	path::{Path, PathBuf},
};

use serde_json::Value;
use tempfile::TempDir;

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
	prelude::{Result, eyre},
	program_intake::{
		self, GoalIntakeIssueAction, GoalIntakeReport, GoalIntakeRunRequest,
		IssueBatchIntakeClassification,
	},
	state::{ExecutionProgramRecord, StateStore},
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueBriefUpdate,
		TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
	},
	workflow::WorkflowDocument,
};

trait TestIssueExt {
	fn with_blocker(self, identifier: &str, state: &str) -> Self;
	fn with_label(self, name: &str) -> Self;
}

#[derive(Default)]
struct FakeTracker {
	issues: RefCell<HashMap<String, TrackerIssue>>,
	next_issue_number: RefCell<usize>,
	created_issues: RefCell<Vec<TrackerIssue>>,
	updated_issues: RefCell<Vec<TrackerIssue>>,
	fail_create_after_successes: RefCell<Option<usize>>,
	fail_update_after_successes: RefCell<Option<usize>>,
}
impl FakeTracker {
	fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
		for issue in issues {
			self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
		}

		self
	}

	fn with_create_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_create_after_successes.borrow_mut() = Some(successes);

		self
	}

	fn with_update_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_update_after_successes.borrow_mut() = Some(successes);

		self
	}

	fn created_issue_count(&self) -> usize {
		self.created_issues.borrow().len()
	}

	fn updated_issue_count(&self) -> usize {
		self.updated_issues.borrow().len()
	}

	fn generated_issue_identifier(&self, index: usize) -> String {
		format!("XY-G{}", index + 1)
	}
}

impl IssueTracker for FakeTracker {
	fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
		Ok(self
			.issues
			.borrow()
			.values()
			.filter(|issue| issue.has_label(label_name))
			.cloned()
			.collect())
	}

	fn find_team_label_id(&self, _team_id: &str, label_name: &str) -> Result<Option<String>> {
		Ok(Some(format!("label-{label_name}")))
	}

	fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
		Ok(self.issues.borrow().get(issue_identifier).cloned())
	}

	fn refresh_issues(&self, _issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		Ok(Vec::new())
	}

	fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> Result<()> {
		Ok(())
	}

	fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn remove_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
		Ok(())
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
		Ok(())
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		if let Some(success_limit) = *self.fail_create_after_successes.borrow()
			&& self.created_issues.borrow().len() >= success_limit
		{
			eyre::bail!("injected create failure after {success_limit} successes");
		}

		let identifier = loop {
			let mut next_issue_number = self.next_issue_number.borrow_mut();

			*next_issue_number += 1;

			let candidate = self.generated_issue_identifier(*next_issue_number - 1);

			if !self.issues.borrow().contains_key(&candidate) {
				break candidate;
			}
		};
		let state_name = request
			.state_id
			.as_deref()
			.and_then(|state_id| state_id.strip_prefix("state-"))
			.unwrap_or("Todo");
		let mut issue = issue(&identifier, state_name);

		issue.id = format!("id-{identifier}");

		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);
		issue.team.id.clone_from(&request.team_id);
		self.issues.borrow_mut().insert(identifier, issue.clone());
		self.created_issues.borrow_mut().push(issue.clone());

		Ok(issue)
	}

	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		if let Some(success_limit) = *self.fail_update_after_successes.borrow()
			&& self.updated_issues.borrow().len() >= success_limit
		{
			eyre::bail!("injected update failure after {success_limit} successes");
		}

		let mut issues = self.issues.borrow_mut();
		let issue = issues
			.values_mut()
			.find(|issue| issue.id == issue_id)
			.ok_or_else(|| eyre::eyre!("issue `{issue_id}` not found"))?;

		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);

		let issue = issue.clone();

		self.updated_issues.borrow_mut().push(issue.clone());

		Ok(issue)
	}
}

impl TestIssueExt for TrackerIssue {
	fn with_blocker(mut self, identifier: &str, state: &str) -> Self {
		self.blockers.push(TrackerIssueBlocker {
			id: format!("id-{identifier}"),
			identifier: identifier.to_owned(),
			state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
		});

		self
	}

	fn with_label(mut self, name: &str) -> Self {
		self.labels.push(TrackerLabel { id: format!("label-{name}"), name: name.to_owned() });

		self
	}
}

#[test]
fn issue_batch_dry_run_classifies_without_persisting() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = workflow();
	let config = test_config();
	let tracker = FakeTracker::default().with_issues([
		issue("XY-1", "Todo"),
		issue("XY-2", "In Progress"),
		issue("XY-3", "Done"),
		issue("XY-4", "Todo")
			.with_blocker("XY-20", "Todo")
			.with_blocker("XY-10", "Todo")
			.with_label("repo:zeta")
			.with_label("repo:alpha"),
	]);
	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![
			String::from("XY-4"),
			String::from("XY-2"),
			String::from("XY-404"),
			String::from("XY-1"),
			String::from("XY-3"),
		],
		true,
		false,
	)
	.expect("dry-run should classify");

	assert_eq!(report.counts.ready, 1);
	assert_eq!(report.counts.held, 1);
	assert_eq!(report.counts.blocked, 1);
	assert_eq!(report.counts.stale, 1);
	assert_eq!(report.counts.unmapped, 1);
	assert_eq!(report.issues[0].issue_identifier, "XY-1");
	assert_eq!(report.issues[0].classification, IssueBatchIntakeClassification::Ready);

	let blocked = report
		.issues
		.iter()
		.find(|issue| issue.issue_identifier == "XY-4")
		.expect("blocked issue should be reported");

	assert_eq!(blocked.blockers, vec![String::from("XY-10"), String::from("XY-20")]);
	assert_eq!(
		blocked.conflict_domains,
		vec![
			String::from("module:alpha"),
			String::from("module:zeta"),
			String::from("tracker_ownership:XY-4"),
		]
	);
	assert!(store.list_execution_programs("decodex").expect("program list should read").is_empty());
}

#[test]
fn project_registration_is_persist_only_for_command_path() {
	let store = StateStore::open_in_memory().expect("store should open");
	let temp_dir = TempDir::new().expect("temp dir should create");
	let config_path = write_project_files(temp_dir.path());

	program_intake::register_intake_project_config_for_persist(&store, &config_path, false)
		.expect("dry-run registration should no-op");

	assert!(store.list_projects().expect("projects should list").is_empty());

	program_intake::register_intake_project_config_for_persist(&store, &config_path, true)
		.expect("persist registration should write");

	let projects = store.list_projects().expect("projects should list");

	assert_eq!(projects.len(), 1);
	assert_eq!(projects[0].service_id(), "decodex");
	assert!(projects[0].enabled());
}

#[test]
fn issue_batch_persist_writes_program_and_adjacent_intake_state() {
	let store = StateStore::open_in_memory().expect("store should open");
	let workflow = workflow();
	let config = test_config();
	let tracker = FakeTracker::default().with_issues([issue("XY-1", "Todo")]);
	let report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("XY-1")],
		false,
		true,
	)
	.expect("persist should write local state");

	assert!(report.persisted);
	assert_eq!(store.list_execution_programs("decodex").expect("programs").len(), 1);
	assert_eq!(store.list_program_intake_plans("decodex").expect("plans").len(), 1);
	assert_eq!(
		store.list_program_issue_mappings("decodex", &report.program_id).expect("mappings").len(),
		1
	);
	assert_eq!(
		store.list_program_intake_plans("decodex").expect("plans")[0].intake_kind(),
		"issue_batch_intake"
	);
}

#[test]
fn goal_intake_dry_run_shows_issue_split_without_mutation() {
	let store = StateStore::open_in_memory().expect("store should open");
	let contract = accepted_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
	let config = test_config();
	let workflow = workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: true,
		apply: false,
	})
	.expect("dry-run should produce materialization plan");

	assert!(report.dry_run);
	assert!(!report.persisted);
	assert_eq!(report.issues.len(), 2);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::WouldCreate);
	assert_eq!(report.issues[0].dependencies, Vec::<String>::new());
	assert_eq!(report.issues[0].conflict_domains, vec![String::from("module:runtime")]);
	assert_eq!(report.issues[1].dependencies, vec![String::from("goal-intake-runtime")]);
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());

	let rendered = program_intake::render_goal_intake_report(&report);

	assert!(rendered.contains("dependencies=none"));
	assert!(rendered.contains("conflict_domains=module:runtime"));
	assert!(rendered.contains("dependencies=goal-intake-runtime"));
}

#[test]
fn goal_intake_refuses_latent_or_missing_decision_authority() {
	let store = StateStore::open_in_memory().expect("store should open");
	let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
	let latent = latent_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), latent)
		.expect("latent contract should persist");

	let config = test_config();
	let workflow = workflow();
	let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("latent contract must not materialize");

	assert!(latent_error.to_string().contains("requires accepted execution authority"));

	let mut needs_decision = latent_goal_contract();

	needs_decision
		.require_human_decision("Choose the public issue split before apply.")
		.expect("contract should record missing decision");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), needs_decision)
		.expect("needs-decision contract should persist");

	let missing_decision_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("missing decision must stop apply");

	assert!(missing_decision_error.to_string().contains("needs_human_decision"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_creates_updates_and_persists_links() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker =
		FakeTracker::default().with_issues([issue("XY-852", "Todo"), issue("XY-G1", "Todo")]);
	let config = test_config();
	let workflow = workflow();
	let report = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect("apply should materialize issues and program");

	assert_goal_intake_apply_report(&report, &tracker);
	assert_goal_intake_runtime_links(&store, &report);

	let updated = tracker
		.get_issue_by_identifier("XY-G1")
		.expect("issue lookup should work")
		.expect("updated issue should exist");

	assert_goal_issue_brief_is_public(&updated.description, &report);
}

#[test]
fn autonomy_proposal_issue_dag_materializes_through_goal_intake_in_isolated_store() {
	let store = StateStore::open_in_memory().expect("isolated store should open");
	let contract = record_accepted_autonomy_dag_decision_contract(&store);
	let tracker = FakeTracker::default().with_issues([issue("XY-2000", "Todo")]);
	let config = test_config();
	let workflow = workflow();
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

	assert_isolated_autonomy_dag_goal_intake(&store, &tracker, &contract, &report);
}

fn record_accepted_autonomy_dag_decision_contract(store: &StateStore) -> DecisionContract {
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

fn assert_isolated_autonomy_dag_goal_intake(
	store: &StateStore,
	tracker: &FakeTracker,
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

fn assert_goal_intake_apply_report(report: &GoalIntakeReport, tracker: &FakeTracker) {
	assert!(report.applied);
	assert!(report.persisted);
	assert_eq!(tracker.updated_issue_count(), 1);
	assert_eq!(tracker.created_issue_count(), 1);
	assert_eq!(report.issues[0].action, GoalIntakeIssueAction::Updated);
	assert_eq!(report.issues[1].action, GoalIntakeIssueAction::Created);
	assert_eq!(report.issues[0].dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(report.issues[1].dispatch_action.as_deref(), None);
	assert!(
		report.issues[1]
			.reasons
			.iter()
			.any(|reason| reason.contains("has not reached a required terminal state"))
	);
}

fn assert_goal_intake_runtime_links(store: &StateStore, report: &GoalIntakeReport) {
	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");
	let node_ids = report.issues.iter().map(|issue| issue.node_id.clone()).collect::<Vec<_>>();

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1"), String::from("XY-G2")]
	);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids(), &node_ids);

	let programs = store
		.list_execution_programs_for_contract("decodex", "goal-intake-contract")
		.expect("programs should list");
	let intake_plans =
		store.list_program_intake_plans("decodex").expect("intake plans should list");

	assert_eq!(programs.len(), 1);
	assert_eq!(programs[0].program_id(), report.program_id);
	assert_eq!(intake_plans.len(), 1);
	assert_eq!(intake_plans[0].intake_kind(), "goal_intake");
	assert_eq!(intake_plans[0].source_contract_id(), Some("goal-intake-contract"));

	assert_goal_intake_plan_lineage(&programs[0]);

	assert_eq!(
		store
			.list_program_issue_mappings("decodex", &report.program_id)
			.expect("mappings should list")
			.len(),
		2
	);
}

fn assert_goal_intake_plan_lineage(record: &ExecutionProgramRecord) {
	let intake_plan = record
		.program()
		.program_intake_plan()
		.expect("program payload should retain intake plan lineage");

	assert_eq!(intake_plan.source_objective_ref(), Some("decodex:quality-autonomy@1"));
	assert_eq!(intake_plan.source_proposal_id(), Some("autonomy_proposal:test-proposal"));
	assert_eq!(intake_plan.source_signal_refs(), &[String::from("autonomy_signal:test-signal")]);
}

fn assert_goal_issue_brief_is_public(description: &str, report: &GoalIntakeReport) {
	for heading in [
		"## Objective",
		"## Authority",
		"## Required Reading",
		"## Ownership Boundary",
		"## Dependencies",
		"## Current-tree Landing Zone",
		"## Acceptance",
		"## Validation",
		"## Lifecycle Gates",
		"## Risk",
		"## Stop Conditions",
	] {
		assert!(description.contains(heading));
	}

	assert!(
		description
			.contains("Accepted Decision Contract authority is recorded in Decodex runtime state.")
	);
	assert!(description.contains("Source issue: `XY-852`"));
	assert!(description.contains("Goal intake dry-run renders generated issue briefs"));
	assert!(description.contains("Use normal Decodex review, PR handoff, landing"));
	assert!(description.contains("Run install or restart steps only when"));
	assert!(description.contains("Stop when promotion authority"));

	assert_goal_issue_brief_hides_private_ids(description, report);
}

fn assert_goal_issue_brief_hides_private_ids(description: &str, report: &GoalIntakeReport) {
	for private_id in [
		"Execution Program: `",
		"Execution Program node:",
		"goal-intake-contract",
		"autonomy_proposal:test-proposal",
		"decodex:quality-autonomy@1",
		"autonomy_signal:test-signal",
		&report.program_id,
		"```",
		"private_evidence_refs",
	] {
		assert!(!description.contains(private_id));
	}
	for issue in &report.issues {
		assert!(!description.contains(&issue.node_id));
	}
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

#[test]
fn generated_issue_text_validation_rejects_private_program_identifiers() {
	let title_error = program_intake::validate_generated_issue_text(
		"Expose goal-decodex-contract-private",
		"## Objective\nUse normal public text.",
		&["goal-decodex-contract-private"],
	)
	.expect_err("title must reject private program id");

	assert!(
		title_error
			.to_string()
			.contains("generated issue title contains a private Program Intake identifier")
	);

	let description_error = program_intake::validate_generated_issue_text(
		"Use normal public text.",
		"## Objective\nExpose goal:contract:01-private-node.",
		&["goal:contract:01-private-node"],
	)
	.expect_err("description must reject private node id");

	assert!(
		description_error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
}

#[test]
fn goal_intake_apply_rejects_generated_briefs_that_leak_autonomy_lineage_ids() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut payload = serde_json::to_value(accepted_goal_contract())
		.expect("accepted goal contract should serialize");

	payload["accepted_authority"]["constraints"]
		.as_array_mut()
		.expect("constraints should be an array")
		.push(serde_json::json!(
			"Do not expose autonomy_signal:test-signal in generated issue text."
		));

	let leaking_contract: DecisionContract =
		serde_json::from_value(payload).expect("leaking contract should deserialize");

	store
		.upsert_decision_contract("decodex", Some("XY-852"), leaking_contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
	let config = test_config();
	let workflow = workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("private autonomy lineage ids must fail generated brief validation");

	assert!(
		error
			.to_string()
			.contains("generated issue description contains a private Program Intake identifier")
	);
	assert_eq!(tracker.created_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_persists_links_after_each_successful_issue_mutation() {
	let store = StateStore::open_in_memory().expect("store should open");
	let contract = accepted_goal_contract();

	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([issue("XY-852", "Todo")])
		.with_create_failure_after_successes(1);
	let config = test_config();
	let workflow = workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("second issue create should fail");

	assert!(error.to_string().contains("injected create failure"));
	assert_eq!(tracker.created_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1")]
	);
	assert_eq!(linked_contract.contract().links().generated_issue_ids().len(), 1);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 1);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_preserves_later_existing_links_after_update_failure() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(
			["id-XY-G1", "id-XY-G2"],
			["XY-G1", "XY-G2"],
			["old-node-1", "old-node-2"],
		)
		.expect("existing generated links should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default()
		.with_issues([issue("XY-852", "Todo"), issue("XY-G1", "Todo"), issue("XY-G2", "Todo")])
		.with_update_failure_after_successes(1);
	let config = test_config();
	let workflow = workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("second issue update should fail");

	assert!(error.to_string().contains("injected update failure"));
	assert_eq!(tracker.updated_issue_count(), 1);

	let linked_contract = store
		.decision_contract("decodex", "goal-intake-contract")
		.expect("contract lookup should read")
		.expect("contract should exist");

	assert_eq!(
		linked_contract.contract().links().generated_issue_identifiers(),
		&[String::from("XY-G1"), String::from("XY-G2")]
	);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids().len(), 2);
	assert_eq!(linked_contract.contract().links().execution_program_node_ids()[1], "old-node-2");
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

#[test]
fn goal_intake_apply_fails_closed_when_existing_generated_link_is_missing() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut contract = accepted_goal_contract();

	contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["old-node"])
		.expect("existing generated link should attach");
	store
		.upsert_decision_contract("decodex", Some("XY-852"), contract)
		.expect("contract should persist");

	let tracker = FakeTracker::default().with_issues([issue("XY-852", "Todo")]);
	let config = test_config();
	let workflow = workflow();
	let error = program_intake::run_goal_intake(GoalIntakeRunRequest {
		state_store: &store,
		tracker: &tracker,
		config: &config,
		workflow: &workflow,
		contract_id: "goal-intake-contract",
		team_issue_identifier: None,
		dry_run: false,
		apply: true,
	})
	.expect_err("missing generated issue link should block apply");

	assert!(error.to_string().contains("Generated issue link `XY-G1`"));
	assert_eq!(tracker.created_issue_count(), 0);
	assert_eq!(tracker.updated_issue_count(), 0);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
}

fn workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(workflow_markdown()).expect("workflow should parse")
}

fn latent_goal_contract() -> DecisionContract {
	serde_json::from_value(latent_goal_contract_payload())
		.expect("goal contract should deserialize")
}

fn latent_goal_contract_payload() -> Value {
	serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": "goal-intake-contract",
		"status": "draft_latent",
		"source_intent": latent_goal_source_intent(),
		"research_provenance": latent_goal_research_provenance(),
		"research_evidence": latent_goal_research_evidence(),
		"research_options": [],
		"accepted_authority": latent_goal_accepted_authority(),
		"execution_readiness": latent_goal_execution_readiness(),
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
		"execution_program_node_ids": []
	},
	"evidence_boundary": {
		"private_evidence_refs": [],
		"public_projection_refs": [],
			"public_summary": "Goal intake contract ready for issue shaping."
		}
	})
}

fn latent_goal_source_intent() -> Value {
	serde_json::json!({
		"summary": "Ship promoted goal intake.",
		"user_utterance": "arrange this goal",
		"source_issue_identifier": "XY-852",
	})
}

fn latent_goal_research_provenance() -> Value {
	serde_json::json!([
		{
			"kind": "autonomy_proposal",
			"reference": "autonomy_proposal:test-proposal",
			"summary": "Accepted autonomy proposal produced this Decision Contract candidate."
		},
		{
			"kind": "autonomy_objective",
			"reference": "decodex:quality-autonomy@1",
			"summary": "Accepted autonomy objective version."
		},
		{
			"kind": "spec",
			"reference": "docs/spec/loop-runtime.md",
			"summary": "Promoted contracts can shape normal Linear issues."
		}
	])
}

fn latent_goal_research_evidence() -> Value {
	serde_json::json!([
		{
			"kind": "autonomy_signal:runtime_health",
			"claim": "Autonomy signal `autonomy_signal:test-signal` contributed.",
			"support": "freshness=fresh; evidence_class=repo_source; confidence=high",
			"source_ref": "autonomy_signal:test-signal"
		},
		{
			"claim": "Goal intake needs generated issues and an internal program.",
			"support": "The loop-runtime spec defines Program Intake records.",
			"source_ref": "docs/spec/loop-runtime.md"
		}
	])
}

fn latent_goal_accepted_authority() -> Value {
	serde_json::json!({
		"accepted_objectives": [
			"Materialize accepted goal intake into normal Linear issues.",
			"Persist the internal Execution Program without exposing graph mechanics."
		],
		"non_goals": ["Do not run implementation from goal intake."],
		"constraints": ["Linear receives only public-safe issue briefs and sparse links."],
		"assumptions": ["The source issue anchors the generated issue team."],
		"objections": [],
		"stop_conditions": [
			"Stop when promotion authority or required decisions are missing."
		]
	})
}

fn latent_goal_execution_readiness() -> Value {
	serde_json::json!({
		"summary": "Ready for issue shaping after promotion.",
		"ready_for_issue_shaping": true,
		"missing_decisions": [],
		"validation_expectations": ["Run cargo make test before handoff."],
		"risk_notes": ["Generated issue descriptions must stay natural-language."],
		"proposed_issues": [goal_intake_runtime_issue(), goal_intake_links_issue()],
		"conflict_domains": ["module:runtime", "file:docs/spec/loop-runtime.md"]
	})
}

fn goal_intake_runtime_issue() -> Value {
	serde_json::json!({
		"key": "goal-intake-runtime",
		"title": "Implement goal intake CLI/API behavior.",
		"objective": "Implement goal intake CLI/API behavior.",
		"stage": "runtime",
		"dependencies": [],
		"conflict_domains": ["module:runtime"],
		"acceptance": ["Goal intake dry-run renders generated issue briefs without mutation."],
		"validation": ["Run cargo make test before handoff."],
		"risk": ["Generated issue descriptions must stay natural-language."],
		"queue_intent": "ready_to_queue"
	})
}

fn goal_intake_links_issue() -> Value {
	serde_json::json!({
		"key": "goal-intake-links",
		"title": "Persist Execution Program links for generated issues.",
		"objective": "Persist Execution Program links for generated issues.",
		"stage": "runtime",
		"dependencies": ["goal-intake-runtime"],
		"conflict_domains": ["module:runtime", "file:docs/spec/loop-runtime.md"],
		"acceptance": [
			"Apply links generated issue identifiers and execution nodes back to the accepted contract."
		],
		"validation": ["Run cargo make test before handoff."],
		"risk": ["Generated issue descriptions must stay natural-language."],
		"queue_intent": "ready_to_queue"
	})
}

fn accepted_goal_contract() -> DecisionContract {
	let mut contract = latent_goal_contract();

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-10T00:00:00Z",
				"conversation",
				Some(String::from("User asked Decodex to arrange this goal.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

fn workflow_markdown() -> &'static str {
	r#"+++
version = 1
[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"
[agent]
transport = "stdio://"
[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]
[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60
[context]
read_first = []
+++
"#
}

fn test_config() -> crate::config::ServiceConfig {
	crate::config::ServiceConfig::parse_toml(
		r#"
service_id = "decodex"
[tracker]
api_key_env_var = "HOME"
[github]
token_env_var = "HOME"
[codex]
review = "standard"
[paths]
repo_root = "."
worktree_root = ".worktrees"
"#,
	)
	.expect("config should parse")
}

fn write_project_files(project_dir: &Path) -> PathBuf {
	fs::write(project_dir.join("WORKFLOW.md"), workflow_markdown()).expect("workflow should write");
	fs::write(
		project_dir.join("project.toml"),
		r#"
service_id = "decodex"
[tracker]
api_key_env_var = "HOME"
[github]
token_env_var = "HOME"
[codex]
review = "standard"
[paths]
repo_root = "."
worktree_root = ".worktrees"
"#,
	)
	.expect("project config should write");

	project_dir.join("project.toml")
}

fn issue(identifier: &str, state: &str) -> TrackerIssue {
	TrackerIssue {
		id: format!("id-{identifier}"),
		identifier: identifier.to_owned(),
		project_slug: None,
		title: format!("Issue {identifier}"),
		author: None,
		description: format!("Implement {identifier}."),
		priority: None,
		created_at: String::from("2026-06-01T00:00:00Z"),
		updated_at: String::from("2026-06-01T00:00:00Z"),
		state: TrackerState { id: format!("state-{state}"), name: state.to_owned() },
		team: TrackerTeam {
			id: String::from("team"),
			name: String::from("Team"),
			states: vec![
				TrackerState { id: String::from("state-Todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-In Progress"),
					name: String::from("In Progress"),
				},
				TrackerState { id: String::from("state-Done"), name: String::from("Done") },
			],
			labels: Vec::new(),
		},
		labels_complete: true,
		labels: Vec::new(),
		blockers: Vec::new(),
	}
}
