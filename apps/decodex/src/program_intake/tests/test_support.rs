use std::path::PathBuf;
use std::{cell::RefCell, collections::HashMap, fs, path::Path};

use serde_json::Value;

use crate::prelude::Result;
use crate::state::ExecutionProgramRecord;
use crate::{
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	prelude::eyre,
	program_intake::{GoalIntakeIssueAction, GoalIntakeReport},
	state::StateStore,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueBriefUpdate,
		TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
	},
	workflow::WorkflowDocument,
};

pub(crate) trait TestIssueExt {
	fn with_blocker(self, identifier: &str, state: &str) -> Self;
	fn with_label(self, name: &str) -> Self;
}

#[derive(Default)]
pub(crate) struct FakeTracker {
	issues: RefCell<HashMap<String, TrackerIssue>>,
	next_issue_number: RefCell<usize>,
	created_issues: RefCell<Vec<TrackerIssue>>,
	updated_issues: RefCell<Vec<TrackerIssue>>,
	fail_create_after_successes: RefCell<Option<usize>>,
	fail_update_after_successes: RefCell<Option<usize>>,
}
impl FakeTracker {
	pub(crate) fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
		for issue in issues {
			self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
		}

		self
	}

	pub(crate) fn with_create_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_create_after_successes.borrow_mut() = Some(successes);

		self
	}

	pub(crate) fn with_update_failure_after_successes(self, successes: usize) -> Self {
		*self.fail_update_after_successes.borrow_mut() = Some(successes);

		self
	}

	pub(crate) fn created_issue_count(&self) -> usize {
		self.created_issues.borrow().len()
	}

	pub(crate) fn updated_issue_count(&self) -> usize {
		self.updated_issues.borrow().len()
	}

	pub(crate) fn generated_issue_identifier(&self, index: usize) -> String {
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

pub(crate) fn assert_goal_intake_apply_report(report: &GoalIntakeReport, tracker: &FakeTracker) {
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

pub(crate) fn assert_goal_intake_runtime_links(store: &StateStore, report: &GoalIntakeReport) {
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

pub(crate) fn assert_goal_issue_brief_is_public(description: &str, report: &GoalIntakeReport) {
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

pub(crate) fn workflow() -> WorkflowDocument {
	WorkflowDocument::parse_markdown(workflow_markdown()).expect("workflow should parse")
}

pub(crate) fn latent_goal_contract() -> DecisionContract {
	serde_json::from_value(latent_goal_contract_payload())
		.expect("goal contract should deserialize")
}

pub(crate) fn accepted_goal_contract() -> DecisionContract {
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

pub(crate) fn test_config() -> crate::config::ServiceConfig {
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

pub(crate) fn write_project_files(project_dir: &Path) -> PathBuf {
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

pub(crate) fn issue(identifier: &str, state: &str) -> TrackerIssue {
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

fn assert_goal_intake_plan_lineage(record: &ExecutionProgramRecord) {
	let intake_plan = record
		.program()
		.program_intake_plan()
		.expect("program payload should retain intake plan lineage");

	assert_eq!(intake_plan.source_objective_ref(), Some("decodex:quality-autonomy@1"));
	assert_eq!(intake_plan.source_proposal_id(), Some("autonomy_proposal:test-proposal"));
	assert_eq!(intake_plan.source_signal_refs(), &[String::from("autonomy_signal:test-signal")]);
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
