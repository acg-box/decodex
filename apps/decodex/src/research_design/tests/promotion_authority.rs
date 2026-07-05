use crate::{
	loop_contract::{DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind},
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

fn sample_promotion() -> DecisionPromotion {
	DecisionPromotion::new(
		"operator",
		DecisionPromotionActorKind::User,
		"2026-06-10T00:00:00Z",
		"conversation",
		Some(String::from("User asked to push this forward.")),
	)
	.expect("sample promotion should validate")
}

#[test]
fn explicit_promotion_grants_execution_authority() {
	let store = StateStore::open_in_memory().expect("store should open");

	research_design::persist_research_design_run(&store, "decodex", decision_ready_input())
		.expect("run should persist");

	let promoted = research_design::promote_research_design_contract(
		&store,
		"decodex",
		"research-design-contract",
		sample_promotion(),
	)
	.expect("promotion should succeed");

	assert_eq!(promoted.status(), DecisionContractStatus::AcceptedPromoted);
	assert!(research_design::ensure_contract_authorizes_execution(&promoted).is_ok());
}

#[test]
fn unaccepted_research_refuses_auto_execution() {
	let store = StateStore::open_in_memory().expect("store should open");

	research_design::persist_research_design_run(&store, "decodex", decision_ready_input())
		.expect("run should persist");

	let record = store
		.decision_contract("decodex", "research-design-contract")
		.expect("contract lookup should work")
		.expect("contract should exist");
	let error = research_design::ensure_contract_authorizes_execution(&record)
		.expect_err("latent research must not authorize execution");

	assert!(
		error.to_string().contains("refusing to create execution work from unaccepted research")
	);
}
