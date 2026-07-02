mod loop_scenarios {
	use std::fs;

	use color_eyre::Report;
	use serde_json::{Value, json};

	use crate::agent::PhaseGoalController;
	use crate::agent::PhaseGoalKind;
	use crate::agent::PhaseGoalSpec;
	use crate::agent::PhaseGoalTransition;
	use crate::agent::ReviewPolicyStopReason;
	use crate::execution_program::ExecutionDispatchAction;
	use crate::execution_program::ExecutionLinearIssueMapping;
	use crate::execution_program::ExecutionProgram;
	use crate::execution_program::ExecutionProgramEvaluation;
	use crate::execution_program::ExecutionProgramNodeStage;
	use crate::execution_program::ExecutionProgramReadinessContext;
	use crate::execution_program::ExecutionQueueIntent;
	use crate::execution_program::ExecutionWorkflowPolicy;
	use crate::loop_contract::DecisionContract;
	use crate::loop_contract::DecisionContractStatus;
	use crate::loop_contract::DecisionPromotion;
	use crate::loop_contract::DecisionPromotionActorKind;
	use crate::orchestrator;
	use crate::orchestrator::AuthorityBoundaryChangedSurface;
	use crate::orchestrator::AuthorityBoundaryCheckInput;
	use crate::orchestrator::AuthorityBoundaryDisposition;
	use crate::orchestrator::AuthorityBoundaryImprovementSignal;
	use crate::orchestrator::AuthorityBoundaryPolicyDecision;
	use crate::orchestrator::AuthorityBoundarySurface;
	use crate::orchestrator::HarnessOutcomeKind;
	use crate::orchestrator::HarnessOutcomeRecordInput;
	use crate::orchestrator::IssueDispatchMode;
	use crate::orchestrator::IssueRunPlan;
	use crate::orchestrator::RepoGateFailure;
	use crate::orchestrator::RepoGateFailureKind;
	use crate::orchestrator::RepoGatePhaseGoalController;
	use crate::state::LoopGuardrailCheckpointInput;
	use crate::state::ReviewPolicyCheckpointInput;
	use crate::state::StateStore;
	use crate::tracker;
	use crate::worktree::WorktreeSpec;

	use crate::orchestrator::tests::{
		commit_worktree_change, loop_guardrail_issue_run, runtime_repo_gate, sample_issue,
		sample_workflow_markdown, temp_project_layout, temp_project_layout_with_workflow_markdown,
	};

	const LOOP_SCENARIO_GATE_SERVICE_ID: &str = "pubfi";

	struct LoopScenarioHarness {
		state_store: StateStore,
		issue_id: &'static str,
		issue_identifier: &'static str,
		run_id: &'static str,
		attempt_number: i64,
	}
	impl LoopScenarioHarness {
		fn new() -> Self {
			Self {
				state_store: StateStore::open_in_memory().expect("state store should open"),
				issue_id: "issue-xy-859",
				issue_identifier: "XY-859",
				run_id: "run-loop-scenario",
				attempt_number: 3,
			}
		}

		fn assert_latent_research_stays_non_executable(&self) -> DecisionContract {
			let contract = loop_scenario_research_x_contract();

			assert_eq!(contract.status(), DecisionContractStatus::DraftLatent);

			self.state_store
				.upsert_decision_contract("decodex", Some(self.issue_identifier), contract.clone())
				.expect("latent contract should persist");

			assert!(
				ExecutionProgram::from_accepted_contract(
					"program-before-promotion",
					"decodex",
					&contract,
					Vec::new(),
				)
				.is_err(),
				"latent research output must not authorize execution"
			);
			assert!(
				self.state_store
					.list_execution_programs_for_contract("decodex", contract.contract_id())
					.expect("programs should load")
					.is_empty(),
				"latent research output must not create executable programs"
			);

			contract
		}

		fn promote_and_evaluate_program(
			&self,
			mut contract: DecisionContract,
		) -> (DecisionContract, ExecutionWorkflowPolicy, ExecutionProgramEvaluation) {
			contract
				.promote(
					DecisionPromotion::new(
						"operator",
						DecisionPromotionActorKind::User,
						"2026-06-10T00:00:00Z",
						"conversation",
						Some(String::from("User promoted the research result for implementation.")),
					)
					.expect("promotion metadata should build"),
				)
				.expect("operator promotion should accept the contract");
			self.state_store
				.upsert_decision_contract("decodex", Some(self.issue_identifier), contract.clone())
				.expect("promoted contract should persist");

			let policy = loop_scenario_decodex_workflow_policy();
			let program = ExecutionProgram::from_accepted_contract(
				"program-loop-scenario",
				"decodex",
				&contract,
				loop_scenario_program_nodes(),
			)
			.expect("accepted contract should shape a program");

			self.state_store
				.upsert_execution_program("decodex", program.clone())
				.expect("program should persist");

			let evaluation = program
				.evaluate(&contract, &policy, &ExecutionProgramReadinessContext::new())
				.expect("program should evaluate");

			(contract, policy, evaluation)
		}

		fn assert_direct_dispatch_shaping(&self, evaluation: &ExecutionProgramEvaluation) {
			let summary = evaluation.operator_summary();

			assert_eq!(evaluation.ready_node_ids(), vec!["node-ready"]);
			assert_eq!(evaluation.dispatchable_node_ids(), vec!["node-ready"]);
			assert_eq!(summary.ready_count, 1);
			assert_eq!(summary.queued_count, 0);
			assert_eq!(summary.blocked_count, 1);
			assert_eq!(summary.mapped_count, 1);
			assert_eq!(summary.held_count, 2);
			assert_eq!(summary.active_count, 1);
			assert_eq!(summary.needs_attention_count, 0);
			assert_eq!(summary.dispatchable_count, 1);
			assert_eq!(
				loop_scenario_dispatch_action(evaluation, "node-ready"),
				Some(ExecutionDispatchAction::Dispatch)
			);
			assert_eq!(loop_scenario_dispatch_action(evaluation, "node-uncovered-direction"), None);
			assert_eq!(loop_scenario_dispatch_action(evaluation, "node-active"), None);
		}

		fn record_review_guardrail_and_assert_harness_feedback(&self, contract: DecisionContract) {
			let private_review_marker =
				self.record_review_checkpoint_and_assert_repair_then_escalation();
			let private_authority_marker =
				self.record_requires_human_authority_boundary(contract.contract_id());

			self.record_uncovered_direction_guardrail();
			self.record_architecture_recovery_exhausted();
			self.state_store
				.record_run_attempt(
					self.run_id,
					self.issue_id,
					self.attempt_number,
					"terminal_guarded",
				)
				.expect("run should persist");
			self.state_store
				.upsert_decision_contract("decodex", Some(self.issue_id), contract)
				.expect("issue-linked contract should persist");

			let recorded = orchestrator::record_harness_outcome_for_issue_run(
				&self.state_store,
				HarnessOutcomeRecordInput {
					project_id: "decodex",
					issue_id: self.issue_id,
					issue_identifier: self.issue_identifier,
					run_id: self.run_id,
					attempt_number: self.attempt_number,
					outcome: HarnessOutcomeKind::ManualAttention,
					error_class: Some("uncovered_direction"),
					validation_result: None,
					pr_url: None,
				},
			)
			.expect("harness outcome should record");

			loop_scenario_assert_harness_candidates(
				recorded.payload(),
				private_review_marker,
				private_authority_marker,
			);
		}

		fn record_architecture_recovery_exhausted(&self) {
			self.state_store
				.append_private_execution_event(
					"decodex",
					self.issue_id,
					self.run_id,
					self.attempt_number,
					"architecture_recovery_terminal",
					json!({
						"schema": "decodex.architecture_recovery_terminal/1",
						"record_version": 1,
						"reason_code": "architecture_recovery_exhausted",
						"guardrail_reason": "validation_repeat",
						"authority_boundary_check_record_id": 99,
						"boundary_disposition": "within_authority",
						"recovery_budget": {
							"attempt": 2,
							"max_attempts": 1,
						},
					}),
				)
				.expect("architecture recovery terminal event should persist");
		}

		fn record_review_checkpoint_and_assert_repair_then_escalation(&self) -> &'static str {
			let private_review_marker = "PRIVATE_REVIEW_PAYLOAD_SHOULD_NOT_SURFACE";
			let review_payload = loop_scenario_review_payload(1, private_review_marker);

			self.state_store
				.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
					project_id: "decodex",
					issue_id: self.issue_id,
					run_id: self.run_id,
					attempt_number: self.attempt_number,
					phase: "handoff",
					review_level: "standard",
					status: "findings",
					head_sha: "0123456789abcdef0123456789abcdef01234567",
					nonclean_rounds: 1,
					details_json: &review_payload.to_string(),
				})
				.expect("review checkpoint should persist");
			self.state_store
				.append_private_execution_event(
					"decodex",
					self.issue_id,
					self.run_id,
					self.attempt_number,
					"review_checkpoint",
					review_payload,
				)
				.expect("review event should persist");
			self.assert_review_checkpoint(1);
			self.record_review_churn_escalation();

			private_review_marker
		}

		fn assert_review_checkpoint(&self, expected_rounds: i64) {
			let checkpoint = self
				.state_store
				.review_policy_checkpoint(
					"decodex",
					self.issue_id,
					self.run_id,
					self.attempt_number,
					"handoff",
				)
				.expect("checkpoint lookup should succeed")
				.expect("review checkpoint should exist");

			assert_eq!(checkpoint.status(), "findings");
			assert_eq!(checkpoint.nonclean_rounds(), expected_rounds);
		}

		fn record_review_churn_escalation(&self) {
			let review_churn_payload = loop_scenario_review_payload(3, "review churn");

			self.state_store
				.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
					project_id: "decodex",
					issue_id: self.issue_id,
					run_id: self.run_id,
					attempt_number: self.attempt_number,
					phase: "handoff",
					review_level: "standard",
					status: "findings",
					head_sha: "0123456789abcdef0123456789abcdef01234567",
					nonclean_rounds: 3,
					details_json: &review_churn_payload.to_string(),
				})
				.expect("review churn checkpoint should persist");
			self.assert_review_checkpoint(3);

			assert_eq!(ReviewPolicyStopReason::Exhausted.error_class(), "review_policy_exhausted");
		}

		fn record_uncovered_direction_guardrail(&self) {
			let uncovered_payload = loop_scenario_uncovered_payload();
			let uncovered_checkpoint = self
				.state_store
				.observe_loop_guardrail_checkpoint(LoopGuardrailCheckpointInput {
					project_id: "decodex",
					issue_id: self.issue_id,
					reason: "uncovered_direction",
					fingerprint: "contract-gap:operator-feedback",
					run_id: self.run_id,
					attempt_number: self.attempt_number,
					details_json: &uncovered_payload.to_string(),
				})
				.expect("uncovered direction checkpoint should persist");

			self.state_store
				.append_private_execution_event(
					"decodex",
					self.issue_id,
					self.run_id,
					self.attempt_number,
					"loop_guardrail_checkpoint",
					uncovered_payload,
				)
				.expect("guardrail event should persist");

			assert_eq!(uncovered_checkpoint.reason(), "uncovered_direction");
			assert_eq!(uncovered_checkpoint.consecutive_count(), 1);
		}

		fn record_requires_human_authority_boundary(&self, contract_id: &str) -> &'static str {
			let private_authority_marker = "PRIVATE_AUTHORITY_PAYLOAD_SHOULD_NOT_SURFACE";
			let event = orchestrator::record_authority_boundary_check_private_event(
			&self.state_store,
			AuthorityBoundaryCheckInput {
				project_id: "decodex",
				issue_id: self.issue_id,
				issue_identifier: self.issue_identifier,
				run_id: self.run_id,
				attempt_number: self.attempt_number,
				decision_contract_ids: vec![contract_id],
				attempted_recovery_reason: "uncovered_direction",
				changed_surfaces: vec![AuthorityBoundaryChangedSurface {
					surface: AuthorityBoundarySurface::Objective,
					change_summary: private_authority_marker,
					policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
					legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
				}],
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				disposition: AuthorityBoundaryDisposition::RequiresHuman,
				final_disposition_reason:
					"Accepted behavior would change and the authority envelope is underspecified.",
				improvement_signals: vec![
					AuthorityBoundaryImprovementSignal {
						kind: "underspecified_decision_contract",
						reason_code: "authority_underspecified",
						target: "decision_contract:research-x-loop-contract",
						recommendation:
							"Add explicit accepted-behavior authority before autonomous recovery.",
					},
					AuthorityBoundaryImprovementSignal {
						kind: "missing_issue_template_field",
						reason_code: "authority_boundary_template_gap",
						target: "issue_template:loop_recovery",
						recommendation:
							"Require issue briefs to name authority-sensitive changed surfaces.",
					},
					AuthorityBoundaryImprovementSignal {
						kind: "missing_validator",
						reason_code: "authority_boundary_validator_gap",
						target: "validator:authority_boundary",
						recommendation:
							"Add a validator that fails recovery when accepted behavior changes.",
					},
				],
			},
		)
		.expect("authority boundary event should persist");

			assert_eq!(event.event_type(), "authority_boundary_check");
			assert_eq!(event.payload()["schema"], "decodex.authority_boundary_check/1");
			assert_eq!(event.payload()["disposition"], "requires_human");
			assert_eq!(event.payload()["issue"]["identifier"], self.issue_identifier);
			assert_eq!(event.payload()["run"]["run_id"], self.run_id);
			assert_eq!(event.payload()["decision_contract_ids"], serde_json::json!([contract_id]));
			assert!(
				self.state_store
					.list_linear_execution_events("decodex", self.issue_id)
					.expect("linear cache should read")
					.is_empty(),
				"authority boundary checks must stay out of the public Linear mirror"
			);

			private_authority_marker
		}
	}

	#[test]
	fn research_to_execution_loop_scenario_shapes_ready_work_and_records_feedback() {
		let harness = LoopScenarioHarness::new();
		let contract = harness.assert_latent_research_stays_non_executable();
		let (contract, _policy, evaluation) = harness.promote_and_evaluate_program(contract);

		harness.assert_direct_dispatch_shaping(&evaluation);
		harness.record_review_guardrail_and_assert_harness_feedback(contract);
	}

	#[test]
	fn loop_scenario_phase_completion_runs_validation_and_guardrails_bound_repair() {
		loop_scenario_assert_phase_goal_completion_runs_validation();
		loop_scenario_assert_validation_guardrail_stops_after_threshold();
	}

	fn loop_scenario_assert_phase_goal_completion_runs_validation() {
		let workflow_markdown = sample_workflow_markdown(
		"pubfi",
		&[],
		"Phase goal validation policy.\n",
		3,
	)
	.replace(
		"canonicalize_commands = []",
		"canonicalize_commands = [\"printf canonicalized > phase-canonicalized.txt\"]",
	)
	.replace(
		"verify_commands = []",
		"verify_commands = [\"test -f phase-canonicalized.txt && printf verified > phase-verified.txt\"]",
	);
		let (_temp_dir, config, workflow) =
			temp_project_layout_with_workflow_markdown(&workflow_markdown);
		let issue = sample_issue(
			"In Progress",
			&[tracker::automation_active_label(LOOP_SCENARIO_GATE_SERVICE_ID).as_str()],
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue_run = IssueRunPlan {
			issue: issue.clone(),
			issue_state: String::from("In Progress"),
			initial_issue_state: String::from("Todo"),
			worktree: WorktreeSpec {
				branch_name: String::from("x/pubfi-pub-101"),
				issue_identifier: issue.identifier.clone(),
				path: config.repo_root().to_path_buf(),
				reused_existing: false,
			},
			retry_project_slug: String::from("pubfi"),
			dispatch_mode: IssueDispatchMode::Normal,
			attempt_number: 1,
			run_id: String::from("pub-101-attempt-1"),
			retry_budget_base: 0,
		};

		commit_worktree_change(config.repo_root(), "ready.txt", "before\n", "add ready file");

		fs::write(config.repo_root().join("ready.txt"), "after\n")
			.expect("tracked diff should write");
		runtime_repo_gate::record_phase_acceptance_progress_checkpoint(
			&config,
			&state_store,
			&issue_run,
			&[],
		);

		let transition = RepoGatePhaseGoalController {
			project: &config,
			workflow: &workflow,
			state_store: &state_store,
			issue_run: &issue_run,
		}
		.phase_goal_completed(PhaseGoalKind::ImplementToValidationReady)
		.expect("completed implementation phase should run validation");
		let events = state_store
			.list_private_execution_events(
				LOOP_SCENARIO_GATE_SERVICE_ID,
				&issue.id,
				&issue_run.run_id,
				1,
			)
			.expect("phase goal events should load");

		assert!(config.repo_root().join("phase-canonicalized.txt").exists());
		assert!(config.repo_root().join("phase-verified.txt").exists());
		assert!(matches!(
			transition,
			PhaseGoalTransition::Continue(PhaseGoalSpec {
				phase: PhaseGoalKind::HandoffEvidence,
				..
			})
		));
		assert!(!matches!(transition, PhaseGoalTransition::CompleteRun));
		assert!(events.iter().any(|event| {
			event.event_type() == "phase_goal_transition"
				&& event.payload()["signal"] == "validation_pass"
		}));
		assert!(events.iter().any(|event| {
			event.event_type() == "phase_goal_next"
				&& event.payload()["phase"] == "handoff_evidence"
		}));
	}

	fn loop_scenario_assert_validation_guardrail_stops_after_threshold() {
		let (_guardrail_temp_dir, guardrail_config, _guardrail_workflow) = temp_project_layout();
		let guardrail_store = StateStore::open_in_memory().expect("guardrail store should open");
		let guardrail_issue = sample_issue("In Progress", &[]);

		for round in 1..=2 {
			let guardrail_issue_run =
				loop_guardrail_issue_run(&guardrail_config, &guardrail_issue, round);
			let stop = orchestrator::retryable_failure_loop_guardrail_stop(
				&guardrail_config,
				&guardrail_store,
				&guardrail_issue_run,
				&loop_scenario_repo_gate_failure(),
			)
			.expect("guardrail observation should persist");

			assert!(stop.is_none(), "round {round} should keep repairing before the threshold");
		}

		let guardrail_issue_run = loop_guardrail_issue_run(&guardrail_config, &guardrail_issue, 3);
		let stop = orchestrator::retryable_failure_loop_guardrail_stop(
			&guardrail_config,
			&guardrail_store,
			&guardrail_issue_run,
			&loop_scenario_repo_gate_failure(),
		)
		.expect("third guardrail observation should persist")
		.expect("third identical failure should stop repair churn");
		let checkpoint = guardrail_store
			.loop_guardrail_checkpoint(
				LOOP_SCENARIO_GATE_SERVICE_ID,
				&guardrail_issue_run.issue.id,
				"validation_repeat",
			)
			.expect("checkpoint lookup should succeed")
			.expect("validation repeat checkpoint should exist");
		let guardrail_events = guardrail_store
			.list_private_execution_events(
				LOOP_SCENARIO_GATE_SERVICE_ID,
				&guardrail_issue_run.issue.id,
				&guardrail_issue_run.run_id,
				guardrail_issue_run.attempt_number,
			)
			.expect("guardrail events should load");

		assert_eq!(stop.reason, orchestrator::LoopGuardrailReason::ValidationRepeat);
		assert_eq!(checkpoint.consecutive_count(), 3);
		assert!(guardrail_events.iter().any(|event| {
			event.event_type() == "loop_guardrail_checkpoint"
				&& event.payload()["reason"] == "validation_repeat"
				&& event.payload()["consecutive_count"] == 3
		}));
	}

	fn loop_scenario_decodex_workflow_policy() -> ExecutionWorkflowPolicy {
		ExecutionWorkflowPolicy::new(
			"decodex",
			vec![String::from("Todo")],
			vec![String::from("Done"), String::from("Canceled")],
			"decodex:manual-only",
			"decodex:needs-attention",
		)
		.expect("policy should build")
	}

	fn loop_scenario_program_nodes() -> Vec<crate::execution_program::ExecutionProgramNode> {
		vec![
			loop_scenario_node(
				"node-ready",
				ExecutionProgramNodeStage::Runtime,
				"Implement the accepted runtime node",
				ExecutionQueueIntent::ReadyToQueue,
				"XY-861",
			),
			loop_scenario_node(
				"node-blocked",
				ExecutionProgramNodeStage::Eval,
				"Validate after the runtime node completes",
				ExecutionQueueIntent::ReadyToQueue,
				"XY-862",
			)
			.with_dependencies(vec![
				crate::execution_program::ExecutionProgramDependency::new("node-ready")
					.expect("dependency should build"),
			])
			.expect("dependency should attach"),
			loop_scenario_node(
				"node-uncovered-direction",
				ExecutionProgramNodeStage::Research,
				"Pause uncovered direction for contract feedback",
				ExecutionQueueIntent::Paused,
				"XY-863",
			),
			loop_scenario_active_node(),
		]
	}

	fn loop_scenario_active_node() -> crate::execution_program::ExecutionProgramNode {
		loop_scenario_node(
			"node-active",
			ExecutionProgramNodeStage::Handoff,
			"Retain handoff ownership without queueing",
			ExecutionQueueIntent::Active,
			"XY-864",
		)
		.with_linear_issue(
			ExecutionLinearIssueMapping::new("linear-node-active", "XY-864", "Todo")
				.expect("issue mapping should build")
				.with_active_label(true),
		)
		.expect("active issue should attach")
	}

	fn loop_scenario_review_payload(nonclean_rounds: i64, private_marker: &'static str) -> Value {
		serde_json::json!({
			"phase": "handoff",
			"status": "findings",
			"head_sha": "0123456789abcdef0123456789abcdef01234567",
			"nonclean_rounds": nonclean_rounds,
			"review": {
				"accepted_findings": [{
					"summary": "Ready node lacks a scenario fixture",
					"evidence": [private_marker]
				}],
				"rejected_findings": [{
					"summary": "Stale comment did not apply to current head"
				}]
			}
		})
	}

	fn loop_scenario_uncovered_payload() -> Value {
		serde_json::json!({
			"reason": "uncovered_direction",
			"fingerprint": "contract-gap:operator-feedback",
			"paused_node_id": "node-uncovered-direction",
			"feedback_target": "decision_contract",
			"other_ready_node_ids": ["node-ready"],
		})
	}

	fn loop_scenario_repo_gate_failure() -> Report {
		Report::new(RepoGateFailure::new(
			RepoGateFailureKind::VerifyCommandFailed,
			String::from("Repo verify command `cargo make test` failed: same assertion failed"),
		))
	}

	fn loop_scenario_assert_harness_candidates(
		payload: &Value,
		private_review_marker: &str,
		private_authority_marker: &str,
	) {
		let candidates = payload["improvement_candidates"]
			.as_array()
			.expect("improvement candidates should be an array");
		let candidate_json =
			serde_json::to_string(candidates).expect("candidate summaries should serialize");

		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "underspecified_decision_contract"
					&& candidate["reason_code"] == "uncovered_direction"
			}),
			"uncovered direction should recommend a contract improvement"
		);
		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "weak_prompt"
					&& candidate["reason_code"] == "accepted_review_findings"
			}),
			"accepted review findings should recommend a prompt or fixture improvement"
		);
		assert_eq!(payload["authority_boundary"]["failed_check_count"], 1);
		assert_eq!(payload["authority_boundary"]["improvement_signal_count"], 3);
		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "underspecified_decision_contract"
					&& candidate["reason_code"] == "authority_underspecified"
			}),
			"authority underspecification should recommend a contract improvement"
		);
		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "missing_issue_template_field"
					&& candidate["reason_code"] == "authority_boundary_template_gap"
			}),
			"authority gaps should recommend issue-template hardening"
		);
		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "missing_validator"
					&& candidate["reason_code"] == "authority_boundary_validator_gap"
			}),
			"authority gaps should recommend validator hardening"
		);
		assert!(
			candidates.iter().any(|candidate| {
				candidate["kind"] == "recovery_budget_exhausted"
					&& candidate["reason_code"] == "architecture_recovery_exhausted"
			}),
			"exhausted architecture recovery should recommend recovery-budget hardening"
		);
		assert!(
			!candidate_json.contains(private_review_marker),
			"harness recommendations must summarize private events without leaking raw payloads"
		);
		assert!(
			!candidate_json.contains(private_authority_marker),
			"authority-boundary recommendations must not leak raw changed-surface payloads"
		);
	}

	fn loop_scenario_research_x_contract() -> DecisionContract {
		serde_json::from_str(include_str!(concat!(
			env!("CARGO_MANIFEST_DIR"),
			"/fixtures/decision_contract/research_x_latent_contract.json"
		)))
		.expect("research X fixture should deserialize")
	}

	fn loop_scenario_node(
		node_id: &str,
		stage: ExecutionProgramNodeStage,
		objective: &str,
		queue_intent: ExecutionQueueIntent,
		issue_identifier: &str,
	) -> crate::execution_program::ExecutionProgramNode {
		crate::execution_program::ExecutionProgramNode::new(node_id, stage, objective, queue_intent)
			.expect("node should build")
			.with_conflict_domains(vec![
				crate::execution_program::ExecutionConflictDomain::new(
					crate::execution_program::ExecutionConflictDomainKind::Module,
					format!("runtime/{node_id}"),
				)
				.expect("conflict domain should build"),
			])
			.expect("conflict domain should attach")
			.with_acceptance_expectations(vec![format!(
				"{issue_identifier} satisfies the promoted contract",
			)])
			.expect("acceptance expectations should attach")
			.with_validation_expectations(vec![format!(
				"{issue_identifier} has deterministic validation",
			)])
			.expect("validation expectations should attach")
			.with_linear_issue(
				ExecutionLinearIssueMapping::new(
					format!("linear-{node_id}"),
					issue_identifier,
					"Todo",
				)
				.expect("issue mapping should build"),
			)
			.expect("issue mapping should attach")
	}

	fn loop_scenario_dispatch_action(
		evaluation: &ExecutionProgramEvaluation,
		node_id: &str,
	) -> Option<ExecutionDispatchAction> {
		evaluation
			.nodes()
			.iter()
			.find(|node| node.node_id() == node_id)
			.unwrap_or_else(|| panic!("missing node evaluation `{node_id}`"))
			.dispatch_action()
	}
}
