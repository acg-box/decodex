mod bridge;
mod challenge;
mod compile;
mod persistence;
mod serde;

use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal,
		AutonomyProposalAcceptedProjectPolicy, AutonomyProposalAuthorityActorKind,
		AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
		AutonomyProposalCompileInput, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalDecisionBridgeAuthorityInput, AutonomyProposalIssueCandidate,
		AutonomyProposalState,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	loop_contract::DecisionContractStatus,
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

pub(in crate::autonomy_proposal::tests) fn accepted_project_policy_fixture(
	objective_id: &str,
	authorized_actor: &str,
	authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
	acceptance_scope: &str,
) -> AutonomyProposalAcceptedProjectPolicy {
	AutonomyProposalAcceptedProjectPolicy {
		project_id: String::from("decodex"),
		objective_id: objective_id.to_owned(),
		objective_version: 1,
		accepted_policy_id: String::from("quality-autonomy-policy"),
		accepted_policy_version: String::from("1"),
		authority_ref: String::from("decodex.runtime_policy:quality-autonomy-policy@1"),
		authorized_actor: authorized_actor.to_owned(),
		authorized_actor_kind,
		authorized_acceptance_sources: vec![acceptance_source.to_owned()],
		authorized_scopes: vec![acceptance_scope.to_owned()],
	}
}

pub(in crate::autonomy_proposal::tests) fn decision_bridge_authority_input(
	accepted_by: &str,
	accepted_by_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
	reason: &str,
	proposal_actor: &str,
	proposal_actor_kind: AutonomyProposalAuthorityActorKind,
	accepted_project_policy: Option<AutonomyProposalAcceptedProjectPolicy>,
) -> AutonomyProposalDecisionBridgeAuthorityInput {
	AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: accepted_by.to_owned(),
		accepted_by_kind,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: acceptance_source.to_owned(),
		reason: reason.to_owned(),
		proposal_actor: proposal_actor.to_owned(),
		proposal_actor_kind,
		accepted_project_policy,
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
	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: String::from("operator"),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::User,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: String::from("conversation"),
		reason: String::from("Operator accepted the proposal for Decision Contract promotion."),
		proposal_actor: String::from("subagent"),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: None,
	})
	.expect("bridge authority should validate")
}

fn accepted_project_policy(
	authorized_actor: &str,
	authorized_actor_kind: AutonomyProposalAuthorityActorKind,
	acceptance_source: &str,
) -> AutonomyProposalAcceptedProjectPolicy {
	accepted_project_policy_fixture(
		"quality-autonomy",
		authorized_actor,
		authorized_actor_kind,
		acceptance_source,
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE,
	)
}

fn runtime_policy_bridge_authority() -> AutonomyProposalDecisionBridgeAuthority {
	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: String::from("subagent"),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_at: String::from("2026-06-22T00:03:00Z"),
		acceptance_source: String::from("runtime-policy"),
		reason: String::from("Accepted project policy allows this agent to accept the proposal."),
		proposal_actor: String::from("subagent"),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: Some(accepted_project_policy(
			"subagent",
			AutonomyProposalAuthorityActorKind::ExternalAgent,
			"runtime-policy",
		)),
	})
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
