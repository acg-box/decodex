use std::{collections::BTreeMap, slice};

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalAcceptedProjectPolicy,
		AutonomyProposalAuthorityActorKind, AutonomyProposalChallengeInput,
		AutonomyProposalChallengeSource, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalIssueCandidate,
		AutonomyProposalRefusalReason, AutonomyProposalState,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	loop_contract::{DecisionContractStatus, DecisionPromotion, DecisionPromotionActorKind},
	state::{DecisionContractRecord, StateStore},
};

trait ExpectNone {
	fn expect_none(self, message: &str);
}

impl<T> ExpectNone for Option<T> {
	fn expect_none(self, message: &str) {
		assert!(self.is_none(), "{message}");
	}
}

fn objective_draft_fixture() -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": 1,
		"state": "draft",
		"summary": "Improve Decodex autonomy quality under explicit authority.",
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src", "docs/spec"],
		"allowed_signal_kinds": ["runtime_health", "review_feedback_cluster"],
		"validation_gates": ["cargo test -p decodex autonomy_proposal --lib"],
		"review_policy": "independent current-head review required",
		"memory_policy": "read-only source-linked memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("draft objective should parse")
}

fn objective_fixture() -> AutonomyObjectiveContract {
	let mut objective = objective_draft_fixture();

	objective
		.accept(
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-22T00:00:00Z",
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept");

	objective
}

fn store_accepted_objective(store: &StateStore) -> AutonomyObjectiveContract {
	store
		.upsert_autonomy_objective_draft("decodex", objective_draft_fixture())
		.expect("objective should store");

	store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-22T00:00:00Z",
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept")
		.objective()
		.clone()
}

fn signal_input() -> AutonomySignalInput {
	AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:runtime-health")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1086")),
		run_id: Some(String::from("xy-1086-attempt-1")),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("3cd19609c44cb18bff9e7a34a2f4853754afcee0")),
		captured_at: String::from("2026-06-22T00:00:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status readback showed repeated friction."),
		evidence: vec![String::from("status readback retained the repeated friction signal")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: vec![String::from("No dashboard comparison included.")],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-22T00:00:05Z"),
	}
}

fn runtime_signal() -> AutonomySignal {
	AutonomySignal::runtime_health(signal_input()).expect("runtime signal should validate")
}

fn compile_input() -> AutonomyProposalCompileInput {
	AutonomyProposalCompileInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_family: String::from("runtime_status"),
		intended_surface: String::from("apps/decodex/src/orchestrator/status.rs"),
		affected_identifiers: vec![
			String::from("OperatorLoopStatus"),
			String::from("operator_status"),
		],
		summary: String::from("Compile a bounded proposal from runtime friction evidence."),
		challenge_requirements: vec![String::from(
			"Subagent or inline skeptic objections are evidence only.",
		)],
		rejected_alternatives: vec![String::from("Direct Decision Contract promotion.")],
		rollback_path: String::from("Discard the dry-run proposal record."),
		weakened_validation_or_review: Vec::new(),
		issue_candidates: Vec::new(),
		created_at: String::from("2026-06-22T00:01:00Z"),
	}
}

fn issue_candidate(
	key: &str,
	stage: &str,
	dependencies: Vec<String>,
) -> AutonomyProposalIssueCandidate {
	AutonomyProposalIssueCandidate {
		key: key.to_owned(),
		title: format!("Issue candidate {key}"),
		objective: format!("Complete issue candidate {key}."),
		stage: stage.to_owned(),
		dependencies,
		conflict_domains: vec![format!("issue:{key}")],
		acceptance: vec![format!("{key} acceptance criterion is met.")],
		validation: vec![String::from("cargo test -p decodex autonomy_proposal --lib")],
		risk: vec![String::from("Keep autonomy proposal non-executable until promotion.")],
		queue_intent: String::from("ready_to_queue"),
	}
}

fn bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(
		"operator",
		AutonomyProposalAuthorityActorKind::User,
		"2026-06-22T00:03:00Z",
		"conversation",
		"Operator accepted the proposal for Decision Contract promotion.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		None,
	)
	.expect("bridge authority should validate")
}

fn accepted_project_policy(
	authorized_actor: &str,
	authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
) -> AutonomyProposalAcceptedProjectPolicy {
	AutonomyProposalAcceptedProjectPolicy::new(
		"decodex",
		"quality-autonomy",
		1,
		"quality-autonomy-policy",
		"1",
		"decodex.runtime_policy:quality-autonomy-policy@1",
		authorized_actor,
		authorized_actor_kind,
		vec![String::from(acceptance_source)],
		vec![String::from(super::AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE)],
	)
	.expect("accepted project policy should validate")
}

fn runtime_policy_bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		"2026-06-22T00:03:00Z",
		"runtime-policy",
		"Accepted project policy allows this agent to accept the proposal.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		Some(accepted_project_policy(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
		)),
	)
	.expect("policy-backed bridge authority should validate")
}

fn store_challenged_autonomy_candidate() -> (StateStore, String, DecisionContractRecord) {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let mut input = compile_input();

	input.affected_identifiers.push(String::from("XY-1087"));

	let mut proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], input)
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	proposal
		.record_challenge(AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::InlineSkeptic,
			actor: String::from("inline"),
			summary: String::from("Inline skeptic found no blocker to latent conversion."),
			objections: Vec::new(),
			evidence_refs: vec![String::from("challenge:inline")],
			recorded_at: String::from("2026-06-22T00:02:00Z"),
		})
		.expect("no-objection challenge should preserve candidate state");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect("accepted proposal should become a latent Decision Contract");

	(store, proposal_id, candidate)
}

fn assert_autonomy_candidate_shape(store: &StateStore, candidate: &DecisionContractRecord) {
	assert_eq!(candidate.status(), DecisionContractStatus::DraftLatent);
	assert!(candidate.contract().promotion().is_none());
	assert_eq!(candidate.source_issue_id(), Some("XY-1087"));
	assert_eq!(candidate.contract().source_intent().source_issue_identifier(), Some("XY-1087"));
	assert!(
		candidate
			.contract()
			.accepted_authority()
			.accepted_objectives()
			.contains(&String::from("Reduce repeated validation and review churn."))
	);
	assert!(
		candidate.contract().accepted_authority().constraints().contains(&String::from(
			"Review requirement: independent current-head review required"
		))
	);
	assert_eq!(
		candidate.contract().execution_readiness().validation_expectations(),
		&[String::from("cargo test -p decodex autonomy_proposal --lib")]
	);
	assert!(
		candidate
			.contract()
			.execution_readiness()
			.risk_notes()
			.contains(&String::from("Evidence gap: No dashboard comparison included."))
	);
	assert_eq!(candidate.contract().execution_readiness().proposed_issues().len(), 1);
	assert!(
		candidate.contract().execution_readiness().proposed_issues()[0]
			.conflict_domains()
			.contains(&String::from("file:apps/decodex/src/orchestrator/status.rs"))
	);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	assert!(store.list_program_intake_plans("decodex").expect("intake plans").is_empty());
}

#[test]
fn autonomy_proposal_challenge_source_accepts_legacy_support_agent_alias() {
	let source: AutonomyProposalChallengeSource =
		serde_json::from_value(serde_json::json!("subagent"))
			.expect("canonical subagent source should parse");
	let legacy_source: AutonomyProposalChallengeSource =
		serde_json::from_value(serde_json::json!("support_agent"))
			.expect("legacy support_agent source should parse");

	assert_eq!(source, AutonomyProposalChallengeSource::Subagent);
	assert!(
		legacy_source == AutonomyProposalChallengeSource::Subagent,
		"legacy support_agent should canonicalize to Subagent"
	);
	assert_eq!(
		serde_json::to_value(legacy_source).expect("source should serialize"),
		serde_json::json!("subagent")
	);
}

#[test]
fn autonomy_proposal_issue_candidate_accepts_mcp_camel_case_fields() {
	let candidate: AutonomyProposalIssueCandidate = serde_json::from_value(serde_json::json!({
		"key": "evaluation-gate",
		"title": "Evaluate the proposed split.",
		"objective": "Prove the proposal split is useful before execution.",
		"stage": "eval",
		"dependencies": ["readback-contract"],
		"conflictDomains": ["module:autonomy"],
		"acceptance": ["Evaluation result is recorded."],
		"validation": ["cargo test -p decodex autonomy_proposal --lib"],
		"risk": ["False positives remain visible."],
		"queueIntent": "ready_to_queue"
	}))
	.expect("MCP-shaped issue candidate should parse");

	assert_eq!(candidate.conflict_domains, [String::from("module:autonomy")]);
	assert_eq!(candidate.queue_intent, "ready_to_queue");
}

#[test]
fn autonomy_proposal_dry_run_candidate_shows_lineage_signals_gates_and_gaps() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
		.expect("proposal should compile");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.objective_id(), "quality-autonomy");
	assert_eq!(proposal.objective_version(), 1);
	assert_eq!(proposal.allowed_surfaces(), ["apps/decodex/src", "docs/spec"]);
	assert_eq!(proposal.validation_gates(), ["cargo test -p decodex autonomy_proposal --lib"]);
	assert_eq!(proposal.source_signal_ids().len(), 1);
	assert_eq!(proposal.gaps(), ["No dashboard comparison included."]);
	assert!(proposal.contradictions().is_empty());
	assert!(proposal.refusal_reasons().is_empty());

	let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

	assert_eq!(dry_run_json["dry_run"], true);
	assert_eq!(dry_run_json["non_executable"], true);
	assert_eq!(dry_run_json["objective_lineage"]["objective_id"], "quality-autonomy");
	assert_eq!(dry_run_json["source_signals"][0]["signal_id"], proposal.source_signal_ids()[0]);
	assert_eq!(dry_run_json["allowed_surfaces"][0], "apps/decodex/src");
	assert_eq!(dry_run_json["goals"][0], "Reduce repeated validation and review churn.");
	assert_eq!(
		dry_run_json["metrics"][0],
		"Validation retry count stays below objective tolerance."
	);
	assert_eq!(dry_run_json["non_goals"][0], "Do not bypass Decision Contract authority.");
	assert_eq!(dry_run_json["review_requirements"][0], "independent current-head review required");
	assert_eq!(
		dry_run_json["challenge_requirements"][0],
		"Subagent or inline skeptic objections are evidence only."
	);
	assert_eq!(dry_run_json["rejected_alternatives"][0], "Direct Decision Contract promotion.");
	assert_eq!(dry_run_json["rollback_path"], "Discard the dry-run proposal record.");
	assert_eq!(
		dry_run_json["validation_gates"][0],
		"cargo test -p decodex autonomy_proposal --lib"
	);
	assert!(dry_run_json["refusal_reasons"].as_array().expect("refusals array").is_empty());
}

#[test]
fn autonomy_proposal_can_carry_explicit_dependent_issue_candidates_into_decision_contract() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let mut input = compile_input();

	input.issue_candidates = vec![
		issue_candidate("readback-contract", "runtime", Vec::new()),
		issue_candidate("evaluation-gate", "eval", vec![String::from("readback-contract")]),
	];

	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], input)
		.expect("proposal with explicit issue candidates should compile");

	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.issue_candidates().len(), 2);

	let contract = proposal
		.to_decision_contract_candidate(bridge_authority())
		.expect("proposal should bridge to latent decision contract");
	let proposed_issues = contract.execution_readiness().proposed_issues();

	assert_eq!(proposed_issues.len(), 2);
	assert_eq!(proposed_issues[0].key(), "readback-contract");
	assert_eq!(proposed_issues[0].stage(), "runtime");
	assert_eq!(proposed_issues[1].key(), "evaluation-gate");
	assert_eq!(proposed_issues[1].stage(), "eval");
	assert_eq!(proposed_issues[1].dependencies(), &[String::from("readback-contract")]);
	assert_eq!(proposed_issues[1].queue_intent(), "ready_to_queue");
}

#[test]
fn autonomy_proposal_rejects_invalid_issue_candidate_dag_shape() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let mut duplicate_input = compile_input();

	duplicate_input.issue_candidates = vec![
		issue_candidate("same-key", "runtime", Vec::new()),
		issue_candidate("same-key", "eval", Vec::new()),
	];

	let duplicate = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		duplicate_input,
	)
	.expect_err("duplicate issue candidate keys should fail");

	assert!(duplicate.to_string().contains("duplicated"));

	let mut missing_dependency_input = compile_input();

	missing_dependency_input.issue_candidates =
		vec![issue_candidate("evaluation-gate", "eval", vec![String::from("missing-runtime")])];

	let missing_dependency = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		missing_dependency_input,
	)
	.expect_err("missing dependency should fail");

	assert!(missing_dependency.to_string().contains("depends on unknown key"));

	let mut cyclic_input = compile_input();

	cyclic_input.issue_candidates = vec![
		issue_candidate("runtime-work", "runtime", vec![String::from("eval-work")]),
		issue_candidate("eval-work", "eval", vec![String::from("runtime-work")]),
	];

	let cyclic =
		AutonomyProposal::compile_dry_run(Some(&objective), slice::from_ref(&signal), cyclic_input)
			.expect_err("cyclic dependencies should fail");

	assert!(cyclic.to_string().contains("cyclic dependencies"));

	let mut self_dependency_input = compile_input();

	self_dependency_input.issue_candidates =
		vec![issue_candidate("self-cycle", "runtime", vec![String::from("self-cycle")])];

	let self_dependency = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		self_dependency_input,
	)
	.expect_err("self dependency should fail");

	assert!(self_dependency.to_string().contains("cyclic dependencies"));

	let mut invalid_stage_input = compile_input();

	invalid_stage_input.issue_candidates =
		vec![issue_candidate("bad-stage", "implementation", Vec::new())];

	let invalid_stage =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], invalid_stage_input)
			.expect_err("unsupported stage should fail");

	assert!(invalid_stage.to_string().contains("unsupported stage"));
}

#[test]
fn autonomy_decision_bridge_accepts_candidate_as_latent_contract_with_lineage_readback() {
	let (store, proposal_id, candidate) = store_challenged_autonomy_candidate();

	assert_autonomy_candidate_shape(&store, &candidate);

	let readback = store
		.decision_contract("decodex", candidate.contract_id())
		.expect("contract readback should work")
		.expect("candidate should persist");

	assert_eq!(readback.contract(), candidate.contract());

	let idempotent = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect("re-accepting the same latent contract should be idempotent");

	assert_eq!(idempotent.contract(), candidate.contract());

	let missing_promotion_authority = DecisionPromotion::new(
		"",
		DecisionPromotionActorKind::User,
		"2026-06-22T00:04:00Z",
		"conversation",
		Some(String::from("User asked Decodex to promote the accepted candidate.")),
	);

	assert!(missing_promotion_authority.is_err());
	assert_eq!(
		store
			.decision_contract("decodex", candidate.contract_id())
			.expect("contract should read")
			.expect("contract should exist")
			.status(),
		DecisionContractStatus::DraftLatent
	);

	let promoted = store
		.promote_decision_contract(
			"decodex",
			candidate.contract_id(),
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-22T00:05:00Z",
				"conversation",
				Some(String::from("User asked Decodex to promote the accepted candidate.")),
			)
			.expect("promotion authority should validate"),
		)
		.expect("valid promotion should use existing Decision Contract semantics");

	assert_eq!(promoted.status(), DecisionContractStatus::AcceptedPromoted);

	let reaccept_after_promote = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect_err("accepted promoted contract must not be overwritten by proposal re-accept");

	assert!(reaccept_after_promote.to_string().contains("will not replace"));
	assert_eq!(
		store
			.decision_contract("decodex", candidate.contract_id())
			.expect("contract should read")
			.expect("contract should exist")
			.status(),
		DecisionContractStatus::AcceptedPromoted
	);
}

#[test]
fn autonomy_decision_bridge_reaccept_refuses_generated_link_replacement() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect("accepted proposal should become a latent Decision Contract");
	let mut linked_contract = candidate.contract().clone();

	linked_contract
		.link_generated_execution_surfaces(["id-XY-G1"], ["XY-G1"], ["node-1"])
		.expect("test contract links should validate");
	store
		.upsert_decision_contract("decodex", candidate.source_issue_id(), linked_contract.clone())
		.expect("linked contract should persist");

	let reaccept_after_links = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			bridge_authority(),
		)
		.expect_err("generated execution links must not be overwritten");

	assert!(reaccept_after_links.to_string().contains("will not replace"));

	let readback = store
		.decision_contract("decodex", candidate.contract_id())
		.expect("contract should read")
		.expect("contract should exist");

	assert_eq!(readback.contract().links().generated_issue_identifiers(), &["XY-G1"]);
	assert_eq!(readback.contract(), &linked_contract);
}

#[test]
fn autonomy_decision_bridge_rejected_and_needs_human_proposals_remain_non_executable() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = store_accepted_objective(&store);
	let signal = store
		.record_autonomy_signal("decodex", runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let mut rejected_input = compile_input();

	rejected_input.intended_surface = String::from("scripts/unowned.rs");

	let rejected = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		rejected_input,
	)
	.expect("rejected proposal should compile");
	let rejected_id = rejected.id().to_owned();

	assert_eq!(rejected.state(), AutonomyProposalState::Rejected);

	store.record_autonomy_proposal("decodex", rejected).expect("rejected proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&rejected_id,
				bridge_authority(),
			)
			.is_err()
	);

	let mut contradiction_input = signal_input();

	contradiction_input.contradictions =
		vec![String::from("Runtime and tracker authority disagree.")];

	let contradiction_signal = store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(contradiction_input).expect("signal should validate"),
		)
		.expect("contradiction signal should store")
		.signal()
		.clone();
	let needs_human = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[contradiction_signal],
		compile_input(),
	)
	.expect("needs-human proposal should compile");
	let needs_human_id = needs_human.id().to_owned();

	assert_eq!(needs_human.state(), AutonomyProposalState::NeedsHumanDecision);

	store
		.record_autonomy_proposal("decodex", needs_human)
		.expect("needs-human proposal should persist");

	assert!(
		store
			.accept_autonomy_proposal_as_decision_contract_candidate(
				"decodex",
				&needs_human_id,
				bridge_authority(),
			)
			.is_err()
	);
	assert!(
		store
			.list_decision_contracts_for_project("decodex")
			.expect("contracts should list")
			.is_empty()
	);
	assert!(store.list_execution_programs("decodex").expect("programs").is_empty());
	assert!(store.list_program_intake_plans("decodex").expect("intake plans").is_empty());
}

fn assert_external_agent_policy_authority_validation() {
	let self_accept_without_policy = AutonomyProposalDecisionBridgeAuthority::new(
		"subagent",
		AutonomyProposalAuthorityActorKind::User,
		"2026-06-22T00:03:00Z",
		"agent-output",
		"Agent accepted its own proposal.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		None,
	);

	assert!(self_accept_without_policy.is_err());

	let wrong_actor_policy = AutonomyProposalDecisionBridgeAuthority::new(
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		"2026-06-22T00:03:00Z",
		"runtime-policy",
		"Agent tried to rely on another actor's policy.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		Some(accepted_project_policy(
			"other-agent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
		)),
	);

	assert!(wrong_actor_policy.is_err());

	let wrong_source_policy = AutonomyProposalDecisionBridgeAuthority::new(
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		"2026-06-22T00:03:00Z",
		"runtime-policy",
		"Agent tried to rely on a policy for a different source.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		Some(accepted_project_policy(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"manual-only",
		)),
	);

	assert!(wrong_source_policy.is_err());

	let missing_acceptance_scope = AutonomyProposalAcceptedProjectPolicy::new(
		"decodex",
		"quality-autonomy",
		1,
		"quality-autonomy-policy",
		"1",
		"decodex.runtime_policy:quality-autonomy-policy@1",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		vec![String::from("runtime-policy")],
		vec![String::from("other_scope")],
	);

	assert!(missing_acceptance_scope.is_err());
}

fn assert_policy_objective_lineage_required(store: &StateStore, proposal_id: &str) {
	let wrong_objective_policy = AutonomyProposalAcceptedProjectPolicy::new(
		"decodex",
		"other-objective",
		1,
		"quality-autonomy-policy",
		"1",
		"decodex.runtime_policy:quality-autonomy-policy@1",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		vec![String::from("runtime-policy")],
		vec![String::from(super::AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE)],
	)
	.expect("wrong objective policy shape should still validate");
	let wrong_objective_authority = AutonomyProposalDecisionBridgeAuthority::new(
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		"2026-06-22T00:03:00Z",
		"runtime-policy",
		"Accepted project policy references the wrong objective.",
		"subagent",
		AutonomyProposalAuthorityActorKind::ExternalAgent,
		Some(wrong_objective_policy),
	)
	.expect("authority validates before proposal lineage is checked");
	let wrong_objective_accept = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			proposal_id,
			wrong_objective_authority,
		)
		.expect_err("policy must match proposal objective lineage");

	assert!(wrong_objective_accept.to_string().contains("does not match proposal"));
}

#[test]
fn autonomy_decision_bridge_external_agent_self_accept_requires_project_policy() {
	assert_external_agent_policy_authority_validation();

	let store = StateStore::open_in_memory().expect("store should open");
	let objective = store_accepted_objective(&store);
	let mut input = signal_input();

	input.source_type = AutonomySignalSourceType::Agent;

	let signal = store
		.record_autonomy_signal(
			"decodex",
			AutonomySignal::runtime_health(input).expect("agent signal should validate"),
		)
		.expect("agent signal should store")
		.signal()
		.clone();
	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	assert_policy_objective_lineage_required(&store, &proposal_id);

	let candidate = store
		.accept_autonomy_proposal_as_decision_contract_candidate(
			"decodex",
			&proposal_id,
			runtime_policy_bridge_authority(),
		)
		.expect("policy-backed external acceptance should bridge to latent contract");

	assert_eq!(candidate.status(), DecisionContractStatus::DraftLatent);
	assert!(candidate.contract().promotion().is_none());
}

#[test]
fn autonomy_proposal_id_ignores_timestamps_signal_order_warning_order_and_challenges() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let mut second_input = signal_input();

	second_input.source_refs = vec![String::from("status:runtime-health:secondary")];
	second_input.evidence = vec![String::from("secondary readback")];

	let second_signal =
		AutonomySignal::runtime_health(second_input).expect("second signal should validate");
	let mut input_a = compile_input();
	let mut input_b = compile_input();

	input_a.affected_identifiers = vec![String::from("b"), String::from("a")];
	input_a.created_at = String::from("2026-06-22T00:01:00Z");
	input_b.affected_identifiers = vec![String::from("a"), String::from("b")];
	input_b.created_at = String::from("2026-06-22T00:55:00Z");

	let proposal_a = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[signal.clone(), second_signal.clone()],
		input_a,
	)
	.expect("proposal a should compile");
	let mut proposal_b = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[second_signal, signal.clone(), signal],
		input_b,
	)
	.expect("proposal b should compile");
	let original_id = proposal_b.id().to_owned();

	proposal_b
		.record_challenge(AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::InlineSkeptic,
			actor: String::from("inline"),
			summary: String::from("Skeptic noted a possible operator wording gap."),
			objections: Vec::new(),
			evidence_refs: vec![String::from("challenge:inline")],
			recorded_at: String::from("2026-06-22T00:56:00Z"),
		})
		.expect("challenge should record");

	assert_eq!(proposal_a.id(), original_id);
	assert_eq!(proposal_a.fingerprint(), proposal_b.fingerprint());
	assert_eq!(proposal_b.id(), original_id);
}

#[test]
fn autonomy_proposal_refusal_reasons_are_specific_and_inspectable() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let missing =
		AutonomyProposal::compile_dry_run(None, slice::from_ref(&signal), compile_input())
			.expect("missing objective proposal should compile as refusal");

	assert_eq!(missing.state(), AutonomyProposalState::NeedsEvidence);
	assert!(missing.has_refusal_reason(AutonomyProposalRefusalReason::MissingObjective));

	let mut stale_input = signal_input();

	stale_input.freshness = AutonomySignalFreshness::Stale;

	let stale_signal =
		AutonomySignal::runtime_health(stale_input).expect("stale signal should validate");
	let stale =
		AutonomyProposal::compile_dry_run(Some(&objective), &[stale_signal], compile_input())
			.expect("stale evidence proposal should compile as refusal");

	assert_eq!(stale.state(), AutonomyProposalState::NeedsEvidence);
	assert!(stale.has_refusal_reason(AutonomyProposalRefusalReason::StaleEvidence));

	let mut contradiction_input = signal_input();

	contradiction_input.contradictions =
		vec![String::from("Tracker says closed while runtime says active.")];

	let contradictory_signal = AutonomySignal::runtime_health(contradiction_input)
		.expect("contradictory signal should validate");
	let contradictory = AutonomyProposal::compile_dry_run(
		Some(&objective),
		&[contradictory_signal],
		compile_input(),
	)
	.expect("contradictory proposal should compile as refusal");

	assert_eq!(contradictory.state(), AutonomyProposalState::NeedsHumanDecision);
	assert!(
		contradictory.has_refusal_reason(AutonomyProposalRefusalReason::UnresolvedContradiction)
	);

	let mut weakened_input = compile_input();

	weakened_input.weakened_validation_or_review =
		vec![String::from("Review evidence is older than the current head.")];

	let weakened = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		weakened_input,
	)
	.expect("weakened validation proposal should compile as refusal");

	assert_eq!(weakened.state(), AutonomyProposalState::NeedsEvidence);
	assert!(weakened.has_refusal_reason(AutonomyProposalRefusalReason::WeakenedValidationReview));

	let mut disallowed_surface_input = compile_input();

	disallowed_surface_input.intended_surface = String::from("scripts/unowned.rs");

	let disallowed_surface = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		disallowed_surface_input,
	)
	.expect("disallowed surface proposal should compile as refusal");

	assert_eq!(disallowed_surface.state(), AutonomyProposalState::Rejected);
	assert!(
		disallowed_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface)
	);

	let mut traversal_surface_input = compile_input();

	traversal_surface_input.intended_surface =
		String::from("apps/decodex/src/../../scripts/unowned.rs");

	let traversal_surface = AutonomyProposal::compile_dry_run(
		Some(&objective),
		slice::from_ref(&signal),
		traversal_surface_input,
	)
	.expect("traversal surface proposal should compile as refusal");

	assert_eq!(traversal_surface.state(), AutonomyProposalState::Rejected);
	assert!(traversal_surface.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSurface));

	let docs_signal =
		AutonomySignal::docs_skill_drift(signal_input()).expect("docs signal should validate");
	let disallowed_kind =
		AutonomyProposal::compile_dry_run(Some(&objective), &[docs_signal], compile_input())
			.expect("disallowed signal proposal should compile as refusal");

	assert_eq!(disallowed_kind.state(), AutonomyProposalState::Rejected);
	assert!(
		disallowed_kind.has_refusal_reason(AutonomyProposalRefusalReason::DisallowedSignalKind)
	);
}

#[test]
fn autonomy_proposal_rejects_promoted_state_without_decision_contract_provenance() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let mut proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
			.expect("proposal should compile");

	proposal.state = AutonomyProposalState::AcceptedPromoted;

	assert!(
		proposal
			.validate()
			.expect_err("accepted_promoted should require promotion provenance")
			.to_string()
			.contains("cannot claim accepted_promoted")
	);
}

#[test]
fn autonomy_proposal_challenge_records_objections_without_acceptance_authority() {
	let objective = objective_fixture();
	let signal = runtime_signal();
	let mut proposal =
		AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
			.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	proposal
		.record_challenge(AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::Subagent,
			actor: String::from("subagent"),
			summary: String::from("Subagent challenged the evidence sufficiency."),
			objections: vec![String::from("Needs a fresher operator status readback.")],
			evidence_refs: vec![String::from("challenge:subagent")],
			recorded_at: String::from("2026-06-22T00:02:00Z"),
		})
		.expect("challenge should record");

	assert_eq!(proposal.id(), proposal_id);
	assert_eq!(proposal.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(proposal.challenge_evidence().len(), 1);
	assert!(!proposal.challenge_evidence()[0].acceptance_authority);
	assert_eq!(
		proposal.challenge_evidence()[0].objections,
		["Needs a fresher operator status readback."]
	);

	let dry_run_json = serde_json::to_value(&proposal).expect("proposal should encode");

	assert_eq!(dry_run_json["challenge_evidence"][0]["acceptance_authority"], false);
	assert_eq!(
		dry_run_json["challenge_evidence"][0]["objections"][0],
		"Needs a fresher operator status readback."
	);

	let candidate = proposal
		.to_decision_contract_candidate(bridge_authority())
		.expect("challenge objections should remain promotion constraints");

	assert!(candidate.accepted_authority().constraints().contains(&String::from(
		"Challenge promotion constraint: Needs a fresher operator status readback."
	)));
	assert!(
		candidate
			.accepted_authority()
			.objections()
			.contains(&String::from("Needs a fresher operator status readback."))
	);
}

#[test]
fn autonomy_proposal_store_round_trips_without_execution_authority_side_effects() {
	let store = StateStore::open_in_memory().expect("store should open");
	let objective = store_accepted_objective(&store);

	objective.supersession().expect_none("accepted fixture must not have supersession");

	let signal = store
		.record_autonomy_signal("decodex", runtime_signal())
		.expect("signal should store")
		.signal()
		.clone();
	let proposal = AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
		.expect("proposal should compile");
	let stored = store
		.record_autonomy_proposal("decodex", proposal.clone())
		.expect("proposal should persist");

	assert_eq!(stored.proposal(), &proposal);
	assert_eq!(
		store
			.autonomy_proposal("decodex", proposal.id())
			.expect("proposal read should work")
			.expect("proposal should exist")
			.proposal(),
		&proposal
	);
	assert!(
		store
			.list_decision_contracts_for_project("decodex")
			.expect("decision contracts should list")
			.is_empty()
	);
	assert!(store.list_execution_programs("decodex").expect("programs should list").is_empty());
	assert!(
		store.list_program_intake_plans("decodex").expect("intake plans should list").is_empty()
	);
}

#[test]
fn autonomy_proposal_sqlite_round_trips_full_dry_run_record() {
	let tempdir = tempfile::tempdir().expect("tempdir should create");
	let db_path = tempdir.path().join("runtime.sqlite3");
	let stored_proposal = {
		let store = StateStore::open(&db_path).expect("store should open");
		let objective = store_accepted_objective(&store);
		let signal = store
			.record_autonomy_signal("decodex", runtime_signal())
			.expect("signal should store")
			.signal()
			.clone();
		let mut proposal =
			AutonomyProposal::compile_dry_run(Some(&objective), &[signal], compile_input())
				.expect("proposal should compile");

		proposal
			.record_challenge(AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::Subagent,
				actor: String::from("subagent"),
				summary: String::from("Subagent challenged the evidence sufficiency."),
				objections: vec![String::from("Needs a fresher operator status readback.")],
				evidence_refs: vec![String::from("challenge:subagent")],
				recorded_at: String::from("2026-06-22T00:02:00Z"),
			})
			.expect("challenge should record");
		store
			.record_autonomy_proposal("decodex", proposal.clone())
			.expect("proposal should persist");

		proposal
	};
	let reopened = StateStore::open(&db_path).expect("store should reopen");
	let readback = reopened
		.autonomy_proposal("decodex", stored_proposal.id())
		.expect("proposal read should work")
		.expect("proposal should exist");

	assert_eq!(readback.proposal(), &stored_proposal);
	assert_eq!(readback.state(), AutonomyProposalState::DecisionCandidate);
	assert_eq!(
		reopened
			.recent_autonomy_proposals_for_project("decodex", 1)
			.expect("recent proposals should list")[0]
			.proposal(),
		&stored_proposal
	);
	assert!(
		reopened
			.list_decision_contracts_for_project("decodex")
			.expect("decision contracts should list")
			.is_empty()
	);
	assert!(reopened.list_execution_programs("decodex").expect("programs should list").is_empty());
	assert!(
		reopened.list_program_intake_plans("decodex").expect("intake plans should list").is_empty()
	);
}
