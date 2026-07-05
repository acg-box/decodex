use crate::research_design::{
	self, ResearchDesignOutcome, ResearchDesignRunInput, ResearchEvidenceInput,
	ResearchOptionInput, ResearchPrivateEvidenceRefInput, ResearchProposedIssueInput,
	ResearchProvenanceInput, ResearchPublicProjectionRefInput, ResearchSubworkInput,
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
fn decision_ready_research_requires_method_gates() {
	let mut missing_options = decision_ready_input();

	missing_options.options.clear();

	let missing_options_error =
		match research_design::compile_research_design_run(missing_options, "decodex") {
			Ok(_) => panic!("decision-ready research should require option comparison"),
			Err(error) => error,
		};

	assert!(missing_options_error.to_string().contains("at least one option comparison"));

	let mut missing_challenge = decision_ready_input();

	missing_challenge.objections.clear();

	let missing_challenge_error =
		match research_design::compile_research_design_run(missing_challenge, "decodex") {
			Ok(_) => panic!("decision-ready research should require a challenge note"),
			Err(error) => error,
		};

	assert!(missing_challenge_error.to_string().contains("recorded challenge objection"));

	let mut missing_validation = decision_ready_input();

	missing_validation.validation_expectations.clear();

	let missing_validation_error =
		match research_design::compile_research_design_run(missing_validation, "decodex") {
			Ok(_) => panic!("decision-ready research should require validation expectations"),
			Err(error) => error,
		};

	assert!(missing_validation_error.to_string().contains("requires validation expectations"));

	let mut missing_evidence_kind = decision_ready_input();

	missing_evidence_kind.evidence[0].kind = String::from("unspecified");

	let missing_evidence_kind_error =
		match research_design::compile_research_design_run(missing_evidence_kind, "decodex") {
			Ok(_) => panic!("decision-ready research should require evidence kinds"),
			Err(error) => error,
		};

	assert!(missing_evidence_kind_error.to_string().contains("requires an evidence kind"));

	let mut missing_promotion_target = decision_ready_input();

	missing_promotion_target.promotion_targets.clear();

	let missing_promotion_target_error =
		match research_design::compile_research_design_run(missing_promotion_target, "decodex") {
			Ok(_) => panic!("decision-ready research should require a promotion target"),
			Err(error) => error,
		};

	assert!(
		missing_promotion_target_error
			.to_string()
			.contains("requires at least one promotion target")
	);
}
