use std::{cell::RefCell, collections::BTreeMap};

use crate::{
	loop_contract::DecisionContract,
	prelude::Result,
	tracker::{
		IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker, TrackerIssueBriefUpdate,
		TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
	},
};

#[derive(Default)]
pub(super) struct DogfoodTracker {
	issues: RefCell<BTreeMap<String, TrackerIssue>>,
	label_additions: RefCell<Vec<(String, Vec<String>)>>,
	label_removals: RefCell<Vec<(String, Vec<String>)>>,
	next_goal_issue_number: RefCell<usize>,
}
impl DogfoodTracker {
	pub(super) fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
		for issue in issues {
			self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
		}

		self
	}

	pub(super) fn upsert_issue(&self, issue: TrackerIssue) {
		self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
	}

	pub(super) fn label_additions(&self) -> Vec<(String, Vec<String>)> {
		self.label_additions.borrow().clone()
	}

	pub(super) fn label_removals(&self) -> Vec<(String, Vec<String>)> {
		self.label_removals.borrow().clone()
	}
}

impl DogfoodTracker {
	fn update_issue_labels(
		&self,
		issue_id: &str,
		label_ids: &[String],
		present: bool,
	) -> Result<()> {
		let mut issues = self.issues.borrow_mut();
		let issue = issues
			.values_mut()
			.find(|issue| issue.id == issue_id)
			.unwrap_or_else(|| panic!("issue `{issue_id}` should exist"));
		let labels = label_ids
			.iter()
			.map(|label_id| {
				issue
					.team
					.labels
					.iter()
					.find(|label| label.id == *label_id)
					.cloned()
					.unwrap_or_else(|| TrackerLabel {
						id: label_id.clone(),
						name: label_id.strip_prefix("label-").unwrap_or(label_id).to_owned(),
					})
			})
			.collect::<Vec<_>>();

		for label in labels {
			if present {
				if !issue.labels.iter().any(|existing| existing.name == label.name) {
					issue.labels.push(label);
				}
			} else {
				issue.labels.retain(|existing| existing.name != label.name);
			}
		}

		Ok(())
	}
}

impl IssueTracker for DogfoodTracker {
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

	fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
		Ok(self
			.issues
			.borrow()
			.values()
			.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
			.cloned()
			.collect())
	}

	fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
		Ok(Vec::new())
	}

	fn update_issue_state(&self, _issue_id: &str, _state_id: &str) -> Result<()> {
		Ok(())
	}

	fn add_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		self.label_additions.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

		self.update_issue_labels(issue_id, label_ids, true)
	}

	fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
		self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

		self.update_issue_labels(issue_id, label_ids, false)
	}

	fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
		Ok(())
	}

	fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
		let identifier = {
			let mut next = self.next_goal_issue_number.borrow_mut();

			*next += 1;

			format!("PUB-G{}", next)
		};
		let state_name = request
			.state_id
			.as_deref()
			.and_then(|state_id| state_id.strip_prefix("state-"))
			.unwrap_or("Todo");
		let mut issue =
			dogfood_issue("pubfi", &format!("issue-{identifier}"), &identifier, state_name, &[]);

		issue.team.id.clone_from(&request.team_id);
		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);
		self.upsert_issue(issue.clone());

		Ok(issue)
	}

	fn update_issue_brief(
		&self,
		issue_id: &str,
		request: &TrackerIssueBriefUpdate,
	) -> Result<TrackerIssue> {
		let mut issues = self.issues.borrow_mut();
		let issue = issues
			.values_mut()
			.find(|issue| issue.id == issue_id)
			.unwrap_or_else(|| panic!("issue `{issue_id}` should exist"));

		issue.title.clone_from(&request.title);
		issue.description.clone_from(&request.description);

		Ok(issue.clone())
	}
}

pub(super) fn dogfood_goal_contract() -> DecisionContract {
	serde_json::from_value(serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": "dogfood-goal-contract",
		"status": "draft_latent",
		"source_intent": {
			"summary": "Dogfood Program Intake.",
			"user_utterance": "arrange this controlled Program Intake fixture",
			"source_issue_identifier": "PUB-940A"
		},
		"research_provenance": [{
			"kind": "spec",
			"reference": "docs/spec/loop-runtime.md",
			"summary": "Program Intake materializes accepted contracts."
		}],
		"research_evidence": [{
			"claim": "Goal intake can create normal issues and persist an Execution Program.",
			"support": "Controlled fixture for XY-942.",
			"source_ref": "XY-942"
		}],
		"research_options": [],
		"accepted_authority": {
			"accepted_objectives": [
				"Dogfood accepted goal intake through generated issues."
			],
			"non_goals": [
				"Do not mutate live production queue labels during this fixture."
			],
			"constraints": [
				"Generated issues remain natural-language Linear briefs."
			],
			"assumptions": [
				"The source issue anchors the generated issue team and state."
			],
			"objections": [],
			"stop_conditions": [
				"Stop if the contract is latent or needs a human decision."
			]
		},
		"execution_readiness": {
			"summary": "Ready for controlled issue shaping.",
			"ready_for_issue_shaping": true,
			"missing_decisions": [],
			"validation_expectations": [
				"Run focused Program Intake E2E tests."
			],
			"risk_notes": [
				"Public readback must stay sparse."
			],
			"proposed_issues": [
				{
					"key": "dogfood-runtime",
					"title": "Dogfood generated runtime issue.",
					"objective": "Dogfood generated runtime issue.",
					"stage": "runtime",
					"dependencies": [],
					"conflict_domains": [
						"module:runtime"
					],
					"acceptance": [
						"Dogfood accepted goal intake through a generated runtime issue."
					],
					"validation": [
						"Run focused Program Intake E2E tests."
					],
					"risk": [
						"Public readback must stay sparse."
					],
					"queue_intent": "ready_to_queue"
				},
				{
					"key": "dogfood-status",
					"title": "Dogfood generated status readback issue.",
					"objective": "Dogfood generated status readback issue.",
					"stage": "runtime",
					"dependencies": [
						"dogfood-runtime"
					],
					"conflict_domains": [
						"module:status"
					],
					"acceptance": [
						"Dogfood accepted goal intake through a generated status readback issue."
					],
					"validation": [
						"Run focused Program Intake E2E tests."
					],
					"risk": [
						"Public readback must stay sparse."
					],
					"queue_intent": "ready_to_queue"
				}
			],
			"conflict_domains": [
				"module:runtime",
				"module:status"
			]
		},
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
			"execution_program_node_ids": []
		},
		"evidence_boundary": {
			"private_evidence_refs": [],
			"public_projection_refs": [],
			"public_summary": "Controlled dogfood goal intake fixture."
		}
	}))
	.expect("dogfood goal contract should deserialize")
}

pub(super) fn dogfood_issue(
	service_id: &str,
	issue_id: &str,
	identifier: &str,
	state_name: &str,
	labels: &[&str],
) -> TrackerIssue {
	let team_labels = vec![
		TrackerLabel {
			id: String::from("label-queued"),
			name: crate::tracker::automation_queue_label(service_id),
		},
		TrackerLabel {
			id: String::from("label-active"),
			name: crate::tracker::automation_active_label(service_id),
		},
		TrackerLabel {
			id: String::from("label-manual"),
			name: String::from("decodex:manual-only"),
		},
		TrackerLabel {
			id: String::from("label-needs-attention"),
			name: String::from("decodex:needs-attention"),
		},
	];
	let issue_labels = labels
		.iter()
		.map(|name| {
			team_labels.iter().find(|label| label.name == *name).cloned().unwrap_or_else(|| {
				TrackerLabel { id: format!("label-{name}"), name: (*name).to_owned() }
			})
		})
		.collect::<Vec<_>>();

	TrackerIssue {
		id: issue_id.to_owned(),
		identifier: identifier.to_owned(),
		project_slug: Some(service_id.to_owned()),
		title: format!("Resolve {identifier}"),
		author: Some(String::from("Decodex")),
		description: format!("Implement controlled Program Intake fixture for {identifier}."),
		priority: None,
		created_at: String::from("2026-06-12T00:00:00Z"),
		updated_at: String::from("2026-06-12T00:00:00Z"),
		state: TrackerState { id: format!("state-{state_name}"), name: state_name.to_owned() },
		team: TrackerTeam {
			id: String::from("team-dogfood"),
			name: String::from("Dogfood"),
			states: vec![
				TrackerState { id: String::from("state-Todo"), name: String::from("Todo") },
				TrackerState {
					id: String::from("state-In Progress"),
					name: String::from("In Progress"),
				},
				TrackerState {
					id: String::from("state-In Review"),
					name: String::from("In Review"),
				},
				TrackerState { id: String::from("state-Done"), name: String::from("Done") },
				TrackerState { id: String::from("state-Canceled"), name: String::from("Canceled") },
				TrackerState {
					id: String::from("state-Duplicate"),
					name: String::from("Duplicate"),
				},
			],
			labels: team_labels,
		},
		labels_complete: true,
		labels: issue_labels,
		blockers: Vec::new(),
	}
}

pub(super) fn with_blocker(
	mut issue: TrackerIssue,
	identifier: &str,
	state_name: &str,
) -> TrackerIssue {
	issue.blockers.push(TrackerIssueBlocker {
		id: format!("issue-blocker-{identifier}"),
		identifier: identifier.to_owned(),
		state: TrackerState {
			id: format!("state-blocker-{state_name}"),
			name: state_name.to_owned(),
		},
	});

	issue
}
