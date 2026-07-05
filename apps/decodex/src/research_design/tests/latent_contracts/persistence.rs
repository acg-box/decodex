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
fn decision_ready_research_persists_latent_contract() {
	let store = StateStore::open_in_memory().expect("store should open");
	let report =
		research_design::persist_research_design_run(&store, "decodex", decision_ready_input())
			.expect("run should persist");

	assert_eq!(report.outcome, ResearchDesignOutcome::DecisionReady);
	assert_eq!(report.contract_status, DecisionContractStatus::DraftLatent);
	assert_eq!(report.source_issue_id.as_deref(), Some("XY-860"));
	assert!(report.ready_for_issue_shaping);
	assert!(report.issue_generation_ready_after_promotion);
	assert!(!report.execution_authority_granted);
	assert_eq!(report.private_evidence_ref_count, 1);
	assert_eq!(report.public_projection_ref_count, 1);

	let record = store
		.decision_contract("decodex", "research-design-contract")
		.expect("contract lookup should work")
		.expect("contract should exist");

	assert_eq!(record.status(), DecisionContractStatus::DraftLatent);
	assert_eq!(record.contract().research_options().len(), 1);
	assert_eq!(
		record.contract().execution_readiness().proposed_issues()[0].key(),
		"research-trigger-plugin"
	);
	assert_eq!(
		report.proposed_issues[0].title(),
		"Wire natural-language research trigger into Decodex plugin surface."
	);
	assert_eq!(
		record.contract().execution_readiness().promotion_targets(),
		&[String::from("plugins/decodex/skills")]
	);
	assert!(
		store
			.list_linear_execution_events("decodex", "XY-860")
			.expect("linear cache should read")
			.is_empty(),
		"research contracts must not mirror private payloads to Linear"
	);
}
