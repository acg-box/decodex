use crate::{
	agent::ReviewPolicyStopReason,
	execution_program::{
		ExecutionDispatchAction, ExecutionProgram, ExecutionProgramEvaluation,
		ExecutionProgramReadinessContext, ExecutionWorkflowPolicy,
	},
	loop_contract::{
		DecisionContract, DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind,
	},
	orchestrator::{
		self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
		AuthorityBoundaryDisposition, AuthorityBoundaryImprovementSignal,
		AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, HarnessOutcomeKind,
		HarnessOutcomeRecordInput,
		tests::runtime_loop_scenarios::loop_scenarios::support::fixtures::{self},
	},
	state::{LoopGuardrailCheckpointInput, ReviewPolicyCheckpointInput, StateStore},
};

pub(crate) const LOOP_SCENARIO_GATE_SERVICE_ID: &str = "pubfi";

pub(crate) struct LoopScenarioHarness {
	state_store: StateStore,
	issue_id: &'static str,
	issue_identifier: &'static str,
	run_id: &'static str,
	attempt_number: i64,
}
impl LoopScenarioHarness {
	pub(crate) fn new() -> Self {
		Self {
			state_store: StateStore::open_in_memory().expect("state store should open"),
			issue_id: "issue-xy-859",
			issue_identifier: "XY-859",
			run_id: "run-loop-scenario",
			attempt_number: 3,
		}
	}

	pub(crate) fn assert_latent_research_stays_non_executable(&self) -> DecisionContract {
		let contract = fixtures::loop_scenario_research_x_contract();

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

	pub(crate) fn promote_and_evaluate_program(
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

		let policy = fixtures::loop_scenario_decodex_workflow_policy();
		let program = ExecutionProgram::from_accepted_contract(
			"program-loop-scenario",
			"decodex",
			&contract,
			fixtures::loop_scenario_program_nodes(),
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

	pub(crate) fn assert_direct_dispatch_shaping(&self, evaluation: &ExecutionProgramEvaluation) {
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
			fixtures::loop_scenario_dispatch_action(evaluation, "node-ready"),
			Some(ExecutionDispatchAction::Dispatch)
		);
		assert_eq!(
			fixtures::loop_scenario_dispatch_action(evaluation, "node-uncovered-direction"),
			None
		);
		assert_eq!(fixtures::loop_scenario_dispatch_action(evaluation, "node-active"), None);
	}

	pub(crate) fn record_review_guardrail_and_assert_harness_feedback(
		&self,
		contract: DecisionContract,
	) {
		let private_review_marker =
			self.record_review_checkpoint_and_assert_repair_then_escalation();
		let private_authority_marker =
			self.record_requires_human_authority_boundary(contract.contract_id());

		self.record_uncovered_direction_guardrail();
		self.record_architecture_recovery_exhausted();
		self.state_store
			.record_run_attempt(self.run_id, self.issue_id, self.attempt_number, "terminal_guarded")
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

		fixtures::loop_scenario_assert_harness_candidates(
			recorded.payload(),
			private_review_marker,
			private_authority_marker,
		);
	}

	pub(crate) fn record_architecture_recovery_exhausted(&self) {
		self.state_store
			.append_private_execution_event(
				"decodex",
				self.issue_id,
				self.run_id,
				self.attempt_number,
				"architecture_recovery_terminal",
				serde_json::json!({
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

	pub(crate) fn record_review_checkpoint_and_assert_repair_then_escalation(
		&self,
	) -> &'static str {
		let private_review_marker = "PRIVATE_REVIEW_PAYLOAD_SHOULD_NOT_SURFACE";
		let review_payload = fixtures::loop_scenario_review_payload(1, private_review_marker);

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

	pub(crate) fn assert_review_checkpoint(&self, expected_rounds: i64) {
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

	pub(crate) fn record_review_churn_escalation(&self) {
		let review_churn_payload = fixtures::loop_scenario_review_payload(3, "review churn");

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

	pub(crate) fn record_uncovered_direction_guardrail(&self) {
		let uncovered_payload = fixtures::loop_scenario_uncovered_payload();
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

	pub(crate) fn record_requires_human_authority_boundary(
		&self,
		contract_id: &str,
	) -> &'static str {
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
					target: "decision_contract:decision-x-loop-contract",
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
