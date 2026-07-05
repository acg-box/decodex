use crate::{
	loop_contract::DecisionContractStatus,
	research_design::{
		self, ResearchDesignOutcome, ResearchDesignRunInput, ResearchEvidenceInput,
		ResearchOptionInput, ResearchPrivateEvidenceRefInput, ResearchProposedIssueInput,
		ResearchProvenanceInput, ResearchPublicProjectionRefInput, ResearchSubworkInput,
	},
	state::StateStore,
};

fn decision_ready_input() -> ResearchDesignRunInput {
	ResearchDesignRunInput {
		contract_id: Some(String::from("research-design-contract")),
		intent: String::from("research Decodex native research runner"),
		source_issue_identifier: Some(String::from("XY-860")),
		outcome: ResearchDesignOutcome::DecisionReady,
		provenance: vec![ResearchProvenanceInput {
			kind: String::from("spec"),
			reference: String::from("docs/spec/loop-runtime.md"),
			summary: String::from("Research output is latent until accepted or promoted."),
		}],
		evidence: vec![ResearchEvidenceInput {
			kind: String::from("repo_source"),
			claim: String::from("Decision-ready research can shape downstream issues."),
			support: String::from(
				"The compiler carries objectives, validation expectations, and structured proposed issues.",
			),
			source_ref: Some(String::from("docs/spec/loop-runtime.md")),
		}],
		options: vec![ResearchOptionInput {
			option: String::from("Compile to Decision Contract"),
			tradeoffs: vec![String::from("Preserves the existing runtime authority boundary.")],
			decision: Some(String::from("Use the existing Decision Contract schema.")),
			rejected_reason: None,
		}],
		ai_subwork: vec![ResearchSubworkInput {
			worker_kind: String::from("scout"),
			objective: String::from("Inspect predecessor contract surfaces."),
			outcome: String::from("Found existing Decision Contract persistence."),
			evidence_refs: vec![String::from("XY-852")],
		}],
		objectives: vec![String::from(
			"Implement a native research/design compiler for Decodex work.",
		)],
		non_goals: vec![String::from("Do not auto-execute latent research.")],
		constraints: vec![String::from("Store private evidence in runtime-local state.")],
		assumptions: vec![String::from(
			"Downstream issue shaping will consume only promoted contracts.",
		)],
		objections: vec![String::from("Promotion must remain explicit.")],
		unresolved_decisions: Vec::new(),
		evidence_gaps: Vec::new(),
		blockers: Vec::new(),
		stop_conditions: vec![String::from("Stop if promotion authority is missing.")],
		readiness_summary: Some(String::from("Ready for issue shaping after explicit promotion.")),
		validation_expectations: vec![String::from("Run the registered Decodex gate.")],
		risk_notes: vec![String::from("Do not expose internal graph mechanics.")],
		proposed_issues: vec![ResearchProposedIssueInput {
			key: String::from("research-trigger-plugin"),
			title: String::from(
				"Wire natural-language research trigger into Decodex plugin surface.",
			),
			objective: String::from(
				"Wire natural-language research trigger into Decodex plugin surface.",
			),
			stage: String::from("plugin"),
			dependencies: Vec::new(),
			conflict_domains: vec![String::from("module:runtime")],
			acceptance: vec![String::from(
				"Natural-language research requests compile into latent Decision Contracts.",
			)],
			validation: vec![String::from("Run the registered Decodex gate.")],
			risk: vec![String::from("Do not expose internal graph mechanics.")],
			queue_intent: String::from("ready_to_queue"),
		}],
		promotion_targets: vec![String::from("plugins/decodex/skills")],
		conflict_domains: vec![String::from("runtime")],
		private_evidence_refs: vec![ResearchPrivateEvidenceRefInput {
			project_id: None,
			issue_id: String::from("XY-860"),
			run_id: String::from("run-860"),
			attempt_number: 1,
			record_id: Some(7),
			event_type: Some(String::from("research_design_result")),
		}],
		public_projection_refs: vec![ResearchPublicProjectionRefInput {
			surface: String::from("linear"),
			reference: String::from("XY-860"),
			summary: String::from("Sparse public issue reference only."),
		}],
		public_summary: Some(String::from("Latent research/design contract.")),
	}
}

#[test]
fn not_decision_ready_research_records_feedback_without_promoting() {
	let store = StateStore::open_in_memory().expect("store should open");
	let mut input = decision_ready_input();

	input.contract_id = Some(String::from("not-ready-contract"));
	input.outcome = ResearchDesignOutcome::NotDecisionReady;

	input.objectives.clear();
	input.proposed_issues.clear();

	input.unresolved_decisions =
		vec![String::from("Choose whether runtime or plugin UX owns first exposure.")];

	let report = research_design::persist_research_design_run(&store, "decodex", input)
		.expect("run should persist");
	let record = store
		.decision_contract("decodex", "not-ready-contract")
		.expect("contract lookup should work")
		.expect("contract should exist");

	assert_eq!(report.outcome, ResearchDesignOutcome::NotDecisionReady);
	assert_eq!(record.status(), DecisionContractStatus::DraftLatent);
	assert!(!record.contract().execution_readiness().ready_for_issue_shaping());
	assert!(report.feedback.contains("must not become implementation work"));
	assert!(research_design::ensure_contract_authorizes_execution(&record).is_err());
}

#[test]
fn blocked_and_needs_human_decision_outcomes_stay_distinct() {
	let mut blocked = decision_ready_input();

	blocked.contract_id = Some(String::from("blocked-contract"));
	blocked.outcome = ResearchDesignOutcome::Blocked;
	blocked.blockers = vec![String::from("Required source is unavailable.")];

	blocked.objectives.clear();
	blocked.evidence.clear();
	blocked.proposed_issues.clear();

	let blocked_report = research_design::persist_research_design_run(
		&StateStore::open_in_memory().expect("store should open"),
		"decodex",
		blocked,
	)
	.expect("blocked run should persist");

	assert_eq!(blocked_report.outcome, ResearchDesignOutcome::Blocked);
	assert_eq!(blocked_report.contract_status, DecisionContractStatus::DraftLatent);
	assert_eq!(blocked_report.blockers, vec![String::from("Required source is unavailable.")]);

	let mut human = decision_ready_input();

	human.contract_id = Some(String::from("human-decision-contract"));
	human.outcome = ResearchDesignOutcome::NeedsHumanDecision;
	human.unresolved_decisions = vec![String::from("Choose the production architecture.")];

	human.objectives.clear();
	human.evidence.clear();
	human.proposed_issues.clear();

	let human_report = research_design::persist_research_design_run(
		&StateStore::open_in_memory().expect("store should open"),
		"decodex",
		human,
	)
	.expect("human decision run should persist");

	assert_eq!(human_report.outcome, ResearchDesignOutcome::NeedsHumanDecision);
	assert_eq!(human_report.contract_status, DecisionContractStatus::NeedsHumanDecision);
	assert_eq!(
		human_report.missing_decisions,
		vec![String::from("Choose the production architecture.")]
	);
}
