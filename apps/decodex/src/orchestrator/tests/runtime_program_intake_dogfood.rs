mod program_intake_dogfood {
	use std::{cell::RefCell, collections::BTreeMap};

	use crate::{
		config::ServiceConfig,
		loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
		orchestrator,
		prelude::Result,
		program_intake::{self, GoalIntakeRunRequest},
		state::StateStore,
		tracker::{
			IssueTracker, TrackerComment, TrackerIssue, TrackerIssueBlocker,
			TrackerIssueBriefUpdate, TrackerIssueCreate, TrackerLabel, TrackerState, TrackerTeam,
		},
		workflow::WorkflowDocument,
	};

	#[derive(Default)]
	struct DogfoodTracker {
		issues: RefCell<BTreeMap<String, TrackerIssue>>,
		label_additions: RefCell<Vec<(String, Vec<String>)>>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
		next_goal_issue_number: RefCell<usize>,
	}
	impl DogfoodTracker {
		fn with_issues(self, issues: impl IntoIterator<Item = TrackerIssue>) -> Self {
			for issue in issues {
				self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
			}

			self
		}

		fn upsert_issue(&self, issue: TrackerIssue) {
			self.issues.borrow_mut().insert(issue.identifier.clone(), issue);
		}

		fn label_additions(&self) -> Vec<(String, Vec<String>)> {
			self.label_additions.borrow().clone()
		}

		fn label_removals(&self) -> Vec<(String, Vec<String>)> {
			self.label_removals.borrow().clone()
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
			let mut issue = dogfood_issue(
				"pubfi",
				&format!("issue-{identifier}"),
				&identifier,
				state_name,
				&[],
			);

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

	#[test]
	fn issue_batch_intake_apply_direct_dispatch_unlock_and_status_readback_is_end_to_end() {
		let (_temp_dir, config, workflow) = super::temp_project_layout();
		let store = StateStore::open_in_memory().expect("state store should open");
		let dependency_todo =
			dogfood_issue(config.service_id(), "issue-dependency", "PUB-942A", "Todo", &[]);
		let dependent_todo = with_blocker(
			dogfood_issue(config.service_id(), "issue-dependent", "PUB-942B", "Todo", &[]),
			"PUB-942A",
			"Todo",
		);
		let tracker = DogfoodTracker::default()
			.with_issues([dependency_todo.clone(), dependent_todo.clone()]);
		let dry_run_report = program_intake::run_issue_batch_intake(
			&store,
			&tracker,
			&config,
			&workflow,
			vec![String::from("PUB-942B"), String::from("PUB-942A")],
			true,
			false,
		)
		.expect("issue-batch dry-run should classify controlled fixtures");

		assert!(dry_run_report.dry_run);
		assert!(!dry_run_report.persisted);
		assert_eq!(dry_run_report.counts.ready, 1);
		assert_eq!(dry_run_report.counts.blocked, 1);
		assert!(store.list_execution_programs(config.service_id()).expect("programs").is_empty());

		let apply_report = program_intake::run_issue_batch_intake(
			&store,
			&tracker,
			&config,
			&workflow,
			vec![String::from("PUB-942B"), String::from("PUB-942A")],
			false,
			true,
		)
		.expect("issue-batch apply should persist the internal program");

		assert!(apply_report.persisted);
		assert!(tracker.label_additions().is_empty());
		assert_eq!(
			store
				.list_program_issue_mappings(config.service_id(), &apply_report.program_id)
				.expect("program issue mappings should list")
				.len(),
			2
		);

		assert_initial_issue_batch_dispatch(&tracker, &config, &workflow, &store, &dependency_todo);

		let mut dependency_done =
			dogfood_issue(config.service_id(), "issue-dependency", "PUB-942A", "Done", &[]);

		dependency_done.id.clone_from(&dependency_todo.id);
		tracker.upsert_issue(dependency_done);

		let mut dependent_unblocked = with_blocker(
			dogfood_issue(config.service_id(), "issue-dependent", "PUB-942B", "Todo", &[]),
			"PUB-942A",
			"Done",
		);

		dependent_unblocked.id.clone_from(&dependent_todo.id);
		tracker.upsert_issue(dependent_unblocked);

		assert_unlocked_issue_batch_dispatch(&tracker, &config, &workflow, &store, &dependent_todo);
	}

	fn assert_initial_issue_batch_dispatch(
		tracker: &DogfoodTracker,
		config: &ServiceConfig,
		workflow: &WorkflowDocument,
		store: &StateStore,
		dependency_todo: &TrackerIssue,
	) {
		let first_selection = orchestrator::select_execution_program_run_candidate_with_summary(
			tracker,
			config,
			workflow,
			store,
			&[],
		)
		.expect("first program scheduler selection should choose only the ready node");
		let selected = first_selection
			.selected
			.expect("ready dependency node should be selected for direct dispatch");

		assert_eq!(first_selection.summary.dispatchable_nodes, 1);
		assert_eq!(selected.issue.id, dependency_todo.id);
		assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
		assert!(tracker.label_additions().is_empty());
		assert!(tracker.label_removals().is_empty());

		let first_snapshot =
			orchestrator::build_live_operator_status_snapshot(tracker, config, workflow, store, 10)
				.expect("first status snapshot should build");
		let first_program =
			first_snapshot.execution_programs.first().expect("program status should surface");

		assert_eq!(first_program.status, "blocked");
		assert_eq!(first_program.intake_kind.as_deref(), Some("issue_batch_intake"));
		assert_eq!(first_program.ready_count, 1);
		assert_eq!(first_program.queued_count, 0);
		assert_eq!(first_program.blocked_count, 1);
		assert!(
			first_program
				.node_readbacks
				.iter()
				.any(|node| node.reason_codes.contains(&String::from("dependency_not_terminal")))
		);
	}

	fn assert_unlocked_issue_batch_dispatch(
		tracker: &DogfoodTracker,
		config: &ServiceConfig,
		workflow: &WorkflowDocument,
		store: &StateStore,
		dependent_todo: &TrackerIssue,
	) {
		let second_selection = orchestrator::select_execution_program_run_candidate_with_summary(
			tracker,
			config,
			workflow,
			store,
			&[],
		)
		.expect("second program scheduler selection should unlock the downstream node");
		let selected = second_selection
			.selected
			.expect("dependent node should be selected for direct dispatch");

		assert_eq!(second_selection.summary.dispatchable_nodes, 1);
		assert_eq!(selected.issue.id, dependent_todo.id);
		assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
		assert!(tracker.label_additions().is_empty());
		assert!(tracker.label_removals().is_empty());

		let second_snapshot =
			orchestrator::build_live_operator_status_snapshot(tracker, config, workflow, store, 10)
				.expect("second status snapshot should build");
		let second_program = second_snapshot
			.execution_programs
			.first()
			.expect("program status should remain visible");
		let rendered_status = orchestrator::render_operator_status(&second_snapshot);

		assert_eq!(second_program.status, "ready", "{rendered_status}");
		assert_eq!(second_program.completed_count, 1);
		assert_eq!(second_program.ready_count, 1);
		assert_eq!(second_program.queued_count, 0);
		assert_eq!(second_program.blocked_count, 0);
		assert!(rendered_status.contains("Execution Programs"));
		assert!(rendered_status.contains("mapped_issues=PUB-942A, PUB-942B"));
		assert!(!rendered_status.contains("private_evidence"));
	}

	#[test]
	fn live_program_status_refreshes_terminal_label_clear_without_scheduler_pass() {
		let (_temp_dir, config, workflow) = super::temp_project_layout();
		let store = StateStore::open_in_memory().expect("state store should open");
		let stale_attention_issue = dogfood_issue(
			config.service_id(),
			"issue-pub-1597",
			"PUB-1597",
			"Todo",
			&["decodex:needs-attention"],
		);
		let tracker = DogfoodTracker::default().with_issues([stale_attention_issue.clone()]);
		let apply_report = program_intake::run_issue_batch_intake(
			&store,
			&tracker,
			&config,
			&workflow,
			vec![String::from("PUB-1597")],
			false,
			true,
		)
		.expect("issue-batch apply should persist the stale mapped issue facts");

		assert_eq!(apply_report.counts.blocked, 1);

		let mut completed_issue =
			dogfood_issue(config.service_id(), "issue-pub-1597", "PUB-1597", "Done", &[]);

		completed_issue.id.clone_from(&stale_attention_issue.id);
		tracker.upsert_issue(completed_issue);

		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker, &config, &workflow, &store, 10,
		)
		.expect("live status should refresh mapped issue facts");
		let program = snapshot.execution_programs.first().expect("program status should surface");
		let rendered_status = orchestrator::render_operator_status(&snapshot);

		assert_eq!(program.program_id, apply_report.program_id);
		assert_eq!(program.status, "completed", "{rendered_status}");
		assert_eq!(program.completed_count, 1);
		assert_eq!(program.needs_attention_count, 0);
		assert_eq!(program.blocked_count, 0);
		assert!(rendered_status.contains("status=completed"), "{rendered_status}");
		assert!(rendered_status.contains("attention=0"), "{rendered_status}");
		assert!(rendered_status.contains("completed=1"), "{rendered_status}");
		assert!(rendered_status.contains("mapped_issues=PUB-1597"), "{rendered_status}");
		assert!(!rendered_status.contains("issue=PUB-1597 issue_state=Todo"));
		assert!(!rendered_status.contains("decodex:needs-attention"));

		let refreshed_program = store
			.list_execution_programs(config.service_id())
			.expect("programs should load")
			.into_iter()
			.find(|record| record.program_id() == apply_report.program_id)
			.expect("refreshed program should remain persisted");
		let refreshed_issue = refreshed_program
			.program()
			.nodes()
			.first()
			.and_then(|node| node.linear_issue())
			.expect("refreshed node should retain its issue mapping");

		assert_eq!(refreshed_issue.issue_state(), "Done");
		assert!(!refreshed_issue.has_needs_attention_label());
	}

	#[test]
	fn goal_intake_rejects_latent_then_apply_direct_dispatch_and_status_readback_is_end_to_end() {
		let (_temp_dir, config, workflow) = super::temp_project_layout();
		let store = StateStore::open_in_memory().expect("state store should open");
		let source_issue =
			dogfood_issue(config.service_id(), "issue-source", "PUB-940A", "Todo", &[]);
		let tracker = DogfoodTracker::default().with_issues([source_issue]);
		let latent = dogfood_goal_contract();

		store
			.upsert_decision_contract(config.service_id(), Some("PUB-940A"), latent.clone())
			.expect("latent contract should persist");

		let latent_error = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: latent.contract_id(),
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect_err("latent contract must not materialize");

		assert!(latent_error.to_string().contains("requires accepted execution authority"));
		assert!(store.list_execution_programs(config.service_id()).expect("programs").is_empty());

		let mut accepted = latent.clone();

		accepted
			.promote(
				DecisionPromotion::new(
					"operator",
					DecisionPromotionActorKind::User,
					"2026-06-12T00:00:00Z",
					"controlled_fixture",
					Some(String::from("XY-942 controlled dogfood accepted authority.")),
				)
				.expect("promotion should build"),
			)
			.expect("contract should promote");
		store
			.upsert_decision_contract(config.service_id(), Some("PUB-940A"), accepted.clone())
			.expect("accepted contract should persist");

		let apply_report = program_intake::run_goal_intake(GoalIntakeRunRequest {
			state_store: &store,
			tracker: &tracker,
			config: &config,
			workflow: &workflow,
			contract_id: accepted.contract_id(),
			team_issue_identifier: None,
			dry_run: false,
			apply: true,
		})
		.expect("accepted contract should materialize issues and program");

		assert!(apply_report.applied);
		assert!(apply_report.persisted);
		assert_eq!(apply_report.issues.len(), 2);
		assert!(tracker.label_additions().is_empty());

		let selection = orchestrator::select_execution_program_run_candidate_with_summary(
			&tracker,
			&config,
			&workflow,
			&store,
			&[],
		)
		.expect("goal-intake program scheduler selection should find generated ready nodes");
		let selected =
			selection.selected.expect("one generated issue should be selected for direct dispatch");

		assert_eq!(selection.summary.dispatchable_nodes, 1);
		assert_eq!(selected.dispatch_mode, orchestrator::IssueDispatchMode::Program);
		assert!(tracker.label_additions().is_empty());
		assert!(tracker.label_removals().is_empty());

		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker, &config, &workflow, &store, 10,
		)
		.expect("goal-intake status snapshot should build");
		let program = snapshot.execution_programs.first().expect("goal program should surface");
		let rendered_status = orchestrator::render_operator_status(&snapshot);

		assert_eq!(program.status, "blocked");
		assert_eq!(program.source_contract_id.as_deref(), Some(accepted.contract_id()));
		assert_eq!(program.intake_kind.as_deref(), Some("goal_intake"));
		assert_eq!(
			program.public_summary.as_deref(),
			Some("Dogfood accepted goal intake through generated issues.")
		);
		assert_eq!(program.ready_count, 1);
		assert_eq!(program.blocked_count, 1);
		assert!(
			program
				.node_readbacks
				.iter()
				.any(|node| node.reason_codes.contains(&String::from("dependency_not_terminal")))
		);
		assert_eq!(program.queued_count, 0);
		assert!(rendered_status.contains("source_contract_id: dogfood-goal-contract"));
		assert!(!rendered_status.contains("private_evidence"));
		assert!(!rendered_status.contains("research_evidence"));
	}

	fn dogfood_goal_contract() -> DecisionContract {
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

	fn dogfood_issue(
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
					TrackerState {
						id: String::from("state-Canceled"),
						name: String::from("Canceled"),
					},
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

	fn with_blocker(mut issue: TrackerIssue, identifier: &str, state_name: &str) -> TrackerIssue {
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
}

use crate::orchestrator::tests::temp_project_layout;
