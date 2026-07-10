use std::{
	cell::RefCell,
	collections::HashMap,
	ffi::CStr,
	fs::{self, File, OpenOptions},
	io::Read as _,
	path::PathBuf,
	sync::{Mutex, MutexGuard, OnceLock},
};

use serde_json::Value;
use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	autonomy_objective::AutonomyObjectiveState,
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE, AutonomyProposal,
		AutonomyProposalAcceptedProjectPolicy, AutonomyProposalAuthorityActorKind,
		AutonomyProposalChallengeInput, AutonomyProposalChallengeSource,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalDecisionBridgeAuthorityInput,
		AutonomyProposalState,
	},
	config::{ProjectAutonomyRuntimePolicyConfig, ServiceConfig},
	execution_program,
	loop_contract::{
		DecisionContractLinks, DecisionContractStatus, DecisionPromotion,
		DecisionPromotionActorKind,
	},
	prelude::{Result, eyre},
	program_intake, runtime,
	state::{AutonomyRuntimePolicyRecord, DecisionContractRecord, StateStore},
	tracker::public_text,
};

thread_local! {
	static AUTHORITY_LOCK_DEPTH: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
}

const RUNTIME_POLICY_ACTOR: &str = "decodex-runtime";
const RUNTIME_POLICY_ACCEPTANCE_SOURCE: &str = "decodex-runtime-policy";
const RUNTIME_POLICY_CHALLENGE_ACTOR: &str = "decodex-runtime-policy-challenger";
const RUNTIME_POLICY_CHALLENGE_EVALUATOR_VERSION: &str = "2";

pub(crate) struct AutonomyAuthorityGuard {
	project_id: String,
	_process: Option<MutexGuard<'static, ()>>,
	_file: Option<File>,
}
impl Drop for AutonomyAuthorityGuard {
	fn drop(&mut self) {
		AUTHORITY_LOCK_DEPTH.with(|depth| {
			let mut depth = depth.borrow_mut();
			let Some(count) = depth.get_mut(&self.project_id) else {
				return;
			};

			*count -= 1;

			if *count == 0 {
				depth.remove(&self.project_id);
			}
		});
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimePolicyPromotionEvaluation {
	pub(crate) contract_id: String,
	pub(crate) objections: Vec<String>,
	pub(crate) execution_authority_granted: bool,
	pub(crate) program_intake_present: bool,
	pub(crate) program_intake_state: RuntimePolicyProgramIntakeState,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimePolicyPromotionOutcome {
	pub(crate) contract: DecisionContractRecord,
	pub(crate) challenge_recorded: bool,
	pub(crate) program_intake_present: bool,
	pub(crate) program_intake_state: RuntimePolicyProgramIntakeState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimePolicyProgramIntakeState {
	Absent,
	Partial,
	Complete,
	Inconsistent,
}
impl RuntimePolicyProgramIntakeState {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Absent => "absent",
			Self::Partial => "partial",
			Self::Complete => "complete",
			Self::Inconsistent => "inconsistent",
		}
	}

	fn is_complete(self) -> bool {
		self == Self::Complete
	}
}

/// Validate the immutable record that would be accepted without persisting it.
pub(crate) fn registered_policy_candidate(
	config: &ServiceConfig,
	store: &StateStore,
	project_id: &str,
	accepted_by: &str,
	accepted_at: &str,
	acceptance_source: &str,
	public_non_goals: Vec<String>,
) -> Result<AutonomyRuntimePolicyRecord> {
	let registered = registered_policy(config, project_id)?;
	let objective_version = objective_version(registered.accepted_objective_version())?;
	let objective = store
		.autonomy_objective(project_id, registered.accepted_objective_id(), objective_version)?
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_objective_missing"))?;

	if objective.state() != AutonomyObjectiveState::Accepted {
		eyre::bail!("autonomy_runtime_policy_objective_not_accepted");
	}

	validate_acceptance_timestamp(accepted_at)?;

	let objective_digest = digest_hex(Sha256::digest(serde_json::to_vec(objective.objective())?));

	if public_non_goals.is_empty() {
		eyre::bail!("autonomy_runtime_policy_public_non_goals_missing");
	}

	public_text::validate_public_text_items("publicNonGoals", &public_non_goals)
		.map_err(|_| eyre::eyre!("autonomy_runtime_policy_public_non_goals_invalid"))?;

	AutonomyRuntimePolicyRecord::new(
		project_id,
		registered.accepted_policy_id(),
		registered.accepted_policy_version(),
		registered.accepted_objective_id(),
		objective_version,
		objective_digest,
		registered.policy_authority_ref(),
		accepted_by,
		accepted_at,
		acceptance_source,
		public_non_goals,
	)
}

pub(crate) fn runtime_policy_candidate_digest(
	policy: &AutonomyRuntimePolicyRecord,
) -> Result<String> {
	let payload = serde_json::to_vec(&serde_json::json!({
		"project_id": policy.project_id(),
		"policy_id": policy.policy_id(),
		"policy_version": policy.policy_version(),
		"objective_id": policy.objective_id(),
		"objective_version": policy.objective_version(),
		"objective_digest": policy.objective_digest(),
		"authority_ref": policy.authority_ref(),
		"accepted_by": policy.accepted_by(),
		"accepted_at": policy.accepted_at(),
		"acceptance_source": policy.acceptance_source(),
		"public_non_goals": policy.public_non_goals(),
	}))?;

	Ok(format!("sha256:{}", digest_hex(Sha256::digest(payload))))
}

/// Evaluate current trusted state without mutating proposal, contract, tracker, or Program Intake.
pub(crate) fn evaluate_registered_policy_promotion(
	config: &ServiceConfig,
	store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> Result<RuntimePolicyPromotionEvaluation> {
	let (policy, proposal) = accepted_policy_and_proposal(config, store, project_id, proposal_id)?;
	let contract_id = proposal.decision_contract_id();
	let objections = internal_challenge_objections(&proposal);
	let existing = store.decision_contract(project_id, &contract_id)?;
	let execution_authority_granted = existing
		.as_ref()
		.is_some_and(|record| record.status() == DecisionContractStatus::AcceptedPromoted);
	let program_intake_state = if execution_authority_granted {
		program_intake_state(store, project_id, &contract_id, existing.as_ref())?
	} else {
		RuntimePolicyProgramIntakeState::Absent
	};

	if let Some(existing) = existing.as_ref() {
		validate_existing_contract(existing, &policy, &proposal)?;
	}

	Ok(RuntimePolicyPromotionEvaluation {
		contract_id,
		objections,
		execution_authority_granted,
		program_intake_present: program_intake_state.is_complete(),
		program_intake_state,
	})
}

/// Run Decodex's internal challenge and promote a Decision Contract only. Program Intake remains
/// a separate typed call so tracker failures cannot be confused with promotion success.
pub(crate) fn apply_registered_policy_promotion(
	config: &ServiceConfig,
	store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> Result<RuntimePolicyPromotionOutcome> {
	let _promotion_lock = acquire_autonomy_project_authority_lock(project_id)?;
	let (policy, proposal) = accepted_policy_and_proposal(config, store, project_id, proposal_id)?;
	let objections = internal_challenge_objections(&proposal);
	let challenge_recorded =
		persist_internal_challenge(store, project_id, &proposal, &policy, objections.clone())?;

	if !objections.is_empty() {
		eyre::bail!("autonomy_runtime_policy_internal_challenge_refused:{}", objections.join(","));
	}

	let contract_id = proposal.decision_contract_id();
	let existing = store.decision_contract(project_id, &contract_id)?;
	let contract = match existing {
		Some(existing) => {
			validate_existing_contract(&existing, &policy, &proposal)?;

			existing
		},
		None => store.accept_autonomy_proposal_as_decision_contract_candidate(
			project_id,
			proposal_id,
			runtime_policy_bridge_authority(&policy, &proposal)?,
		)?,
	};
	let promoted = match contract.status() {
		DecisionContractStatus::AcceptedPromoted => contract,
		DecisionContractStatus::DraftLatent => store.promote_decision_contract(
			project_id,
			contract.contract_id(),
			expected_runtime_policy_promotion(&policy, &proposal)?,
		)?,
		DecisionContractStatus::NeedsHumanDecision => {
			eyre::bail!("autonomy_runtime_policy_contract_needs_human_decision")
		},
		DecisionContractStatus::RejectedSuperseded => {
			eyre::bail!("autonomy_runtime_policy_contract_rejected_or_superseded")
		},
	};
	let program_intake_state =
		program_intake_state(store, project_id, promoted.contract_id(), Some(&promoted))?;

	Ok(RuntimePolicyPromotionOutcome {
		contract: promoted,
		challenge_recorded,
		program_intake_present: program_intake_state.is_complete(),
		program_intake_state,
	})
}

pub(crate) fn acquire_autonomy_project_authority_lock(
	project_id: &str,
) -> Result<AutonomyAuthorityGuard> {
	let nested = AUTHORITY_LOCK_DEPTH.with(|depth| {
		let mut depth = depth.borrow_mut();

		if let Some(count) = depth.get_mut(project_id) {
			*count += 1;

			true
		} else {
			false
		}
	});

	if nested {
		return Ok(AutonomyAuthorityGuard {
			project_id: project_id.to_owned(),
			_process: None,
			_file: None,
		});
	}

	let process_guard = authority_process_mutex()
		.lock()
		.map_err(|_| eyre::eyre!("autonomy_authority_process_lock_poisoned"))?;
	let lock_root = runtime::decodex_home_dir()?.join("locks");

	fs::create_dir_all(&lock_root)?;

	let digest = digest_hex(Sha256::digest(project_id.as_bytes()));
	let path: PathBuf = lock_root.join(format!("autonomy-authority-{digest}.lock"));
	let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;

	file.lock()?;
	AUTHORITY_LOCK_DEPTH.with(|depth| {
		depth.borrow_mut().insert(project_id.to_owned(), 1);
	});

	Ok(AutonomyAuthorityGuard {
		project_id: project_id.to_owned(),
		_process: Some(process_guard),
		_file: Some(file),
	})
}

pub(crate) fn program_intake_state_for_contract(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<RuntimePolicyProgramIntakeState> {
	let contract = store.decision_contract(project_id, contract_id)?;

	program_intake_state(store, project_id, contract_id, contract.as_ref())
}

pub(crate) fn ensure_contract_proposal_still_eligible(
	config: &ServiceConfig,
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
) -> Result<()> {
	let contract = store
		.decision_contract(project_id, contract_id)?
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_intake_contract_missing"))?;
	let runtime_policy_contract = contract.contract().promotion().is_some_and(|promotion| {
		promotion.accepted_by_kind() == DecisionPromotionActorKind::RuntimePolicy
	});
	let proposal = store.autonomy_proposal_for_decision_contract(project_id, contract_id)?;
	let Some(proposal) = proposal else {
		if runtime_policy_contract {
			eyre::bail!("autonomy_runtime_policy_intake_source_proposal_missing");
		}

		return Ok(());
	};

	if runtime_policy_contract {
		let (policy, current_proposal) =
			accepted_policy_and_proposal(config, store, project_id, proposal.proposal().id())?;

		validate_existing_contract(&contract, &policy, &current_proposal)?;
	}

	let objections = internal_challenge_objections(proposal.proposal());

	if !objections.is_empty() {
		eyre::bail!("autonomy_runtime_policy_intake_refused:{}", objections.join(","));
	}

	Ok(())
}

pub(crate) fn resolved_local_principal() -> Result<String> {
	let entry = unsafe { libc::getpwuid(libc::geteuid()) };

	if entry.is_null() {
		eyre::bail!("runtime_policy_principal_missing");
	}

	let name = unsafe { CStr::from_ptr((*entry).pw_name) }
		.to_str()
		.map_err(|_| eyre::eyre!("runtime_policy_principal_invalid"))?;

	if name.trim().is_empty() {
		eyre::bail!("runtime_policy_principal_missing");
	}

	Ok(name.to_owned())
}

pub(crate) fn new_operator_receipt_id() -> Result<String> {
	let mut bytes = [0_u8; 32];
	let mut random = File::open("/dev/urandom")?;

	random.read_exact(&mut bytes)?;

	Ok(format!("runtime-policy-receipt-{}", digest_hex(bytes)))
}

pub(crate) fn current_rfc3339() -> Result<String> {
	now_rfc3339()
}

pub(crate) fn operator_receipt_expiry_unix() -> i64 {
	(OffsetDateTime::now_utc() + Duration::minutes(10)).unix_timestamp()
}

fn authority_process_mutex() -> &'static Mutex<()> {
	static AUTHORITY_PROCESS_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

	AUTHORITY_PROCESS_MUTEX.get_or_init(|| Mutex::new(()))
}

fn registered_policy<'a>(
	config: &'a ServiceConfig,
	project_id: &str,
) -> Result<&'a ProjectAutonomyRuntimePolicyConfig> {
	if config.service_id() != project_id {
		eyre::bail!("autonomy_runtime_policy_project_mismatch");
	}
	if !config.autonomy().auto_promote() {
		eyre::bail!("autonomy_auto_promote_disabled");
	}

	config.autonomy().runtime_policy().ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_missing"))
}

fn accepted_policy_and_proposal(
	config: &ServiceConfig,
	store: &StateStore,
	project_id: &str,
	proposal_id: &str,
) -> Result<(AutonomyRuntimePolicyRecord, AutonomyProposal)> {
	let registered = registered_policy(config, project_id)?;
	let policy = store
		.autonomy_runtime_policy(
			project_id,
			registered.accepted_policy_id(),
			registered.accepted_policy_version(),
		)?
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_not_accepted"))?;

	if policy.authority_ref() != registered.policy_authority_ref()
		|| policy.objective_id() != registered.accepted_objective_id()
		|| policy.objective_version() != objective_version(registered.accepted_objective_version())?
	{
		eyre::bail!("autonomy_runtime_policy_registered_binding_mismatch");
	}

	let objective = store
		.autonomy_objective(project_id, policy.objective_id(), policy.objective_version())?
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_objective_missing"))?;

	if objective.state() != AutonomyObjectiveState::Accepted {
		eyre::bail!("autonomy_runtime_policy_objective_not_accepted");
	}

	let objective_digest = digest_hex(Sha256::digest(serde_json::to_vec(objective.objective())?));

	if policy.objective_digest() != objective_digest {
		eyre::bail!("autonomy_runtime_policy_objective_digest_mismatch");
	}

	let proposal = store
		.autonomy_proposal(project_id, proposal_id)?
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_proposal_missing"))?
		.proposal()
		.clone();

	if proposal.project_id() != project_id
		|| proposal.objective_id() != policy.objective_id()
		|| proposal.objective_version() != policy.objective_version()
	{
		eyre::bail!("autonomy_runtime_policy_objective_lineage_mismatch");
	}

	Ok((policy, proposal))
}

fn internal_challenge_objections(proposal: &AutonomyProposal) -> Vec<String> {
	let mut objections = Vec::new();

	if proposal.state() != AutonomyProposalState::DecisionCandidate {
		objections.push("proposal_not_decision_candidate".to_owned());
	}
	if !proposal.refusal_reasons().is_empty() {
		objections.push("proposal_has_refusal_reasons".to_owned());
	}
	if proposal.source_signal_ids().is_empty() {
		objections.push("source_signal_missing".to_owned());
	}
	if !proposal.contradictions().is_empty() {
		objections.push("unresolved_contradictions".to_owned());
	}
	if !proposal.gaps().is_empty() {
		objections.push("unresolved_evidence_gaps".to_owned());
	}
	if proposal.allowed_surfaces().is_empty() {
		objections.push("allowed_surface_missing".to_owned());
	}
	if proposal.validation_gates().is_empty() {
		objections.push("validation_gate_missing".to_owned());
	}
	if proposal.review_requirements().is_empty() {
		objections.push("review_requirement_missing".to_owned());
	}
	if proposal.challenge_requirements().is_empty() {
		objections.push("challenge_requirement_missing".to_owned());
	}
	if proposal.rollback_path().trim().is_empty() {
		objections.push("rollback_path_missing".to_owned());
	}
	if proposal.issue_candidates().is_empty() {
		objections.push("issue_candidate_missing".to_owned());
	}

	for candidate in proposal.issue_candidates() {
		if candidate.queue_intent() != "ready_to_queue" {
			objections.push(format!("issue_candidate_not_ready:{}", candidate.key()));
		}
	}
	for challenge in proposal.challenge_evidence() {
		objections.extend(
			challenge
				.objections()
				.iter()
				.map(|objection| format!("recorded_challenge:{objection}")),
		);
	}

	objections.sort();
	objections.dedup();

	objections
}

fn persist_internal_challenge(
	store: &StateStore,
	project_id: &str,
	proposal: &AutonomyProposal,
	policy: &AutonomyRuntimePolicyRecord,
	objections: Vec<String>,
) -> Result<bool> {
	let challenge_ref = runtime_policy_challenge_ref(policy, proposal)?;
	let already_recorded = proposal.challenge_evidence().iter().any(|challenge| {
		challenge.actor() == RUNTIME_POLICY_CHALLENGE_ACTOR
			&& challenge.evidence_refs().iter().any(|reference| reference == &challenge_ref)
	});

	if already_recorded {
		return Ok(false);
	}

	store.record_autonomy_proposal_challenge(
		project_id,
		proposal.id(),
		AutonomyProposalChallengeInput {
			source: AutonomyProposalChallengeSource::InlineSkeptic,
			actor: RUNTIME_POLICY_CHALLENGE_ACTOR.to_owned(),
			summary: if objections.is_empty() {
				"Decodex internal runtime-policy challenge found no blocking objection.".to_owned()
			} else {
				"Decodex internal runtime-policy challenge refused promotion.".to_owned()
			},
			objections,
			evidence_refs: vec![challenge_ref, policy.authority_ref().to_owned()],
			recorded_at: now_rfc3339()?,
		},
	)?;

	Ok(true)
}

fn runtime_policy_challenge_ref(
	policy: &AutonomyRuntimePolicyRecord,
	proposal: &AutonomyProposal,
) -> Result<String> {
	let mut payload = serde_json::to_value(proposal)?;
	let object = payload
		.as_object_mut()
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_proposal_shape_invalid"))?;
	let challenge_evidence = object
		.get_mut("challenge_evidence")
		.and_then(Value::as_array_mut)
		.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_challenge_evidence_shape_invalid"))?;

	challenge_evidence.retain(|challenge| {
		challenge.get("actor").and_then(Value::as_str) != Some(RUNTIME_POLICY_CHALLENGE_ACTOR)
	});

	let digest = digest_hex(Sha256::digest(serde_json::to_vec(&payload)?));

	Ok(format!(
		"decodex:runtime-policy-challenge/{}/{}/{}/{RUNTIME_POLICY_CHALLENGE_EVALUATOR_VERSION}",
		policy.policy_id(),
		policy.policy_version(),
		digest
	))
}

fn expected_runtime_policy_promotion(
	policy: &AutonomyRuntimePolicyRecord,
	proposal: &AutonomyProposal,
) -> Result<DecisionPromotion> {
	DecisionPromotion::new(
		policy.authority_ref(),
		DecisionPromotionActorKind::RuntimePolicy,
		policy.accepted_at(),
		RUNTIME_POLICY_ACCEPTANCE_SOURCE,
		Some(format!(
			"Accepted policy {}@{} promoted proposal {} after Decodex internal challenge.",
			policy.policy_id(),
			policy.policy_version(),
			proposal.id(),
		)),
	)
}

fn runtime_policy_bridge_authority(
	policy: &AutonomyRuntimePolicyRecord,
	proposal: &AutonomyProposal,
) -> Result<AutonomyProposalDecisionBridgeAuthority> {
	let accepted_policy = AutonomyProposalAcceptedProjectPolicy {
		project_id: policy.project_id().to_owned(),
		objective_id: policy.objective_id().to_owned(),
		objective_version: policy.objective_version(),
		accepted_policy_id: policy.policy_id().to_owned(),
		accepted_policy_version: policy.policy_version().to_owned(),
		authority_ref: policy.authority_ref().to_owned(),
		authorized_actor: RUNTIME_POLICY_ACTOR.to_owned(),
		authorized_actor_kind: AutonomyProposalAuthorityActorKind::RuntimePolicy,
		authorized_acceptance_sources: vec![RUNTIME_POLICY_ACCEPTANCE_SOURCE.to_owned()],
		authorized_scopes: vec![AUTONOMY_PROPOSAL_ACCEPTANCE_SCOPE.to_owned()],
		public_non_goals: policy.public_non_goals().to_vec(),
	};

	AutonomyProposalDecisionBridgeAuthority::new(AutonomyProposalDecisionBridgeAuthorityInput {
		accepted_by: RUNTIME_POLICY_ACTOR.to_owned(),
		accepted_by_kind: AutonomyProposalAuthorityActorKind::RuntimePolicy,
		accepted_at: policy.accepted_at().to_owned(),
		acceptance_source: RUNTIME_POLICY_ACCEPTANCE_SOURCE.to_owned(),
		reason: format!(
			"Accepted policy {} authorized proposal {} after Decodex internal challenge.",
			policy.authority_ref(),
			proposal.id(),
		),
		proposal_actor: "external-autonomy-bridge".to_owned(),
		proposal_actor_kind: AutonomyProposalAuthorityActorKind::ExternalAgent,
		accepted_project_policy: Some(accepted_policy),
	})
}

fn validate_existing_contract(
	record: &DecisionContractRecord,
	policy: &AutonomyRuntimePolicyRecord,
	proposal: &AutonomyProposal,
) -> Result<()> {
	let expected = proposal
		.to_decision_contract_candidate(runtime_policy_bridge_authority(policy, proposal)?)?;
	let mut expected_payload = serde_json::to_value(expected)?;
	let mut actual_payload = serde_json::to_value(record.contract())?;

	for payload in [&mut expected_payload, &mut actual_payload] {
		let object = payload
			.as_object_mut()
			.ok_or_else(|| eyre::eyre!("autonomy_runtime_policy_contract_shape_invalid"))?;

		object.remove("status");
		object.remove("promotion");
		object.remove("links");
	}

	if actual_payload != expected_payload {
		eyre::bail!("autonomy_runtime_policy_existing_contract_identity_mismatch");
	}

	match record.status() {
		DecisionContractStatus::AcceptedPromoted => {
			let promotion = record.contract().promotion().ok_or_else(|| {
				eyre::eyre!("autonomy_runtime_policy_promotion_provenance_missing")
			})?;

			if promotion != &expected_runtime_policy_promotion(policy, proposal)? {
				eyre::bail!("autonomy_runtime_policy_existing_contract_authority_mismatch");
			}
		},
		DecisionContractStatus::DraftLatent => {},
		DecisionContractStatus::NeedsHumanDecision => {
			eyre::bail!("autonomy_runtime_policy_contract_needs_human_decision")
		},
		DecisionContractStatus::RejectedSuperseded => {
			eyre::bail!("autonomy_runtime_policy_contract_rejected_or_superseded")
		},
	}

	Ok(())
}

fn program_intake_state(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
	contract: Option<&DecisionContractRecord>,
) -> Result<RuntimePolicyProgramIntakeState> {
	let expected_program_id = program_intake::goal_program_id(project_id, contract_id);
	let programs = store.list_execution_programs_for_contract(project_id, contract_id)?;
	let plans = store.list_program_intake_plans(project_id)?;
	let expected_plans = plans
		.iter()
		.filter(|candidate| candidate.program_id() == expected_program_id.as_str())
		.collect::<Vec<_>>();
	let expected_mappings = store.list_program_issue_mappings(project_id, &expected_program_id)?;
	let contract_plans = plans
		.iter()
		.filter(|candidate| candidate.source_contract_id() == Some(contract_id))
		.collect::<Vec<_>>();
	let Some(contract) = contract else {
		return Ok(absent_or_inconsistent(
			programs.is_empty()
				&& contract_plans.is_empty()
				&& expected_plans.is_empty()
				&& expected_mappings.is_empty(),
		));
	};
	let links = contract.contract().links();
	let link_lengths = [
		links.generated_issue_ids().len(),
		links.generated_issue_identifiers().len(),
		links.execution_program_node_ids().len(),
	];
	let no_links = link_lengths.iter().all(|length| *length == 0);
	let partial_links = link_lengths.contains(&0) && !no_links;

	if no_links {
		return Ok(absent_or_inconsistent(
			programs.is_empty()
				&& contract_plans.is_empty()
				&& expected_plans.is_empty()
				&& expected_mappings.is_empty(),
		));
	}
	if programs.is_empty() {
		return Ok(if expected_plans.is_empty() && expected_mappings.is_empty() {
			RuntimePolicyProgramIntakeState::Partial
		} else {
			RuntimePolicyProgramIntakeState::Inconsistent
		});
	}
	if partial_links || programs.len() != 1 {
		return Ok(RuntimePolicyProgramIntakeState::Inconsistent);
	}

	let program = &programs[0];
	let payload = program.program();
	let Some(plan) = payload.program_intake_plan() else {
		return Ok(RuntimePolicyProgramIntakeState::Inconsistent);
	};
	let expected_fingerprint =
		execution_program::decision_contract_fingerprint(contract.contract())?;
	let matching_plans = expected_plans
		.iter()
		.filter(|candidate| candidate.program_id() == program.program_id())
		.copied()
		.collect::<Vec<_>>();

	if program.source_contract_id() != Some(contract_id)
		|| payload.source_contract_id() != Some(contract_id)
		|| plan.source_contract_id() != Some(contract_id)
		|| plan.accepted_contract_fingerprint() != expected_fingerprint
		|| matching_plans.len() != 1
		|| matching_plans[0].project_id() != project_id
		|| matching_plans[0].plan_id() != plan.plan_id()
		|| matching_plans[0].intake_kind() != plan.intake_kind().as_str()
		|| matching_plans[0].source_contract_id() != Some(contract_id)
		|| matching_plans[0].accepted_contract_fingerprint() != expected_fingerprint
		|| matching_plans[0].public_summary() != plan.public_summary()
	{
		return Ok(RuntimePolicyProgramIntakeState::Inconsistent);
	}

	let mappings = store.list_program_issue_mappings(project_id, program.program_id())?;
	let mut expected_issue_ids = Vec::new();
	let mut expected_issue_identifiers = Vec::new();
	let mut expected_node_ids = Vec::new();

	for node in payload.nodes() {
		expected_node_ids.push(node.node_id().to_owned());

		let Some(issue) = node.linear_issue() else {
			return Ok(RuntimePolicyProgramIntakeState::Inconsistent);
		};

		expected_issue_ids.push(issue.issue_id().to_owned());
		expected_issue_identifiers.push(issue.issue_identifier().to_owned());

		if !mappings.iter().any(|mapping| {
			mapping.node_id() == node.node_id()
				&& mapping.issue_id() == issue.issue_id()
				&& mapping.issue_identifier() == issue.issue_identifier()
				&& mapping.issue_state() == issue.issue_state()
				&& mapping.queue_intent() == node.queue_intent().as_str()
		}) {
			return Ok(RuntimePolicyProgramIntakeState::Inconsistent);
		}
	}

	Ok(
		if exact_program_intake_links_match(
			links,
			mappings.len(),
			payload.nodes().len(),
			expected_issue_ids,
			expected_issue_identifiers,
			expected_node_ids,
		) {
			RuntimePolicyProgramIntakeState::Complete
		} else {
			RuntimePolicyProgramIntakeState::Inconsistent
		},
	)
}

fn exact_program_intake_links_match(
	links: &DecisionContractLinks,
	mapping_count: usize,
	node_count: usize,
	mut expected_issue_ids: Vec<String>,
	mut expected_issue_identifiers: Vec<String>,
	mut expected_node_ids: Vec<String>,
) -> bool {
	for values in [&mut expected_issue_ids, &mut expected_issue_identifiers, &mut expected_node_ids]
	{
		values.sort();
		values.dedup();
	}

	let mut actual_issue_ids = links.generated_issue_ids().to_vec();
	let mut actual_issue_identifiers = links.generated_issue_identifiers().to_vec();
	let mut actual_node_ids = links.execution_program_node_ids().to_vec();

	actual_issue_ids.sort();
	actual_issue_identifiers.sort();
	actual_node_ids.sort();

	mapping_count == node_count
		&& actual_issue_ids == expected_issue_ids
		&& actual_issue_identifiers == expected_issue_identifiers
		&& actual_node_ids == expected_node_ids
}

fn absent_or_inconsistent(all_state_absent: bool) -> RuntimePolicyProgramIntakeState {
	if all_state_absent {
		RuntimePolicyProgramIntakeState::Absent
	} else {
		RuntimePolicyProgramIntakeState::Inconsistent
	}
}

fn digest_hex(digest: impl IntoIterator<Item = u8>) -> String {
	let mut encoded = String::with_capacity(64);

	for byte in digest {
		encoded.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		encoded.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	encoded
}

fn objective_version(value: &str) -> Result<u64> {
	let version = value
		.parse::<u64>()
		.map_err(|_| eyre::eyre!("autonomy_runtime_policy_objective_version_invalid"))?;

	if version == 0 {
		eyre::bail!("autonomy_runtime_policy_objective_version_invalid");
	}

	Ok(version)
}

fn now_rfc3339() -> Result<String> {
	Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn validate_acceptance_timestamp(value: &str) -> Result<()> {
	let accepted_at = OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| eyre::eyre!("autonomy_runtime_policy_accepted_at_invalid"))?;
	let now = OffsetDateTime::now_utc();

	if accepted_at > now + Duration::minutes(5) || accepted_at < now - Duration::days(30) {
		eyre::bail!("autonomy_runtime_policy_accepted_at_out_of_bounds");
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{sync::mpsc, thread, time::Duration};

	use tempfile::TempDir;

	use crate::{autonomy_runtime_policy, test_support::TestEnvVarGuard};

	#[test]
	fn authority_lock_is_reentrant_and_serializes_same_process_threads() {
		let home = TempDir::new().expect("temporary home should create");
		let _home = TestEnvVarGuard::set(
			"HOME",
			home.path().to_str().expect("temporary home should be UTF-8"),
		);
		let outer = autonomy_runtime_policy::acquire_autonomy_project_authority_lock("PUB")
			.expect("outer authority lock should acquire");
		let nested = autonomy_runtime_policy::acquire_autonomy_project_authority_lock("PUB")
			.expect("nested authority lock should be reentrant");

		drop(nested);

		let (attempting_tx, attempting_rx) = mpsc::channel();
		let (acquired_tx, acquired_rx) = mpsc::channel();
		let contender = thread::spawn(move || {
			attempting_tx.send(()).expect("attempt signal should send");

			let guard = autonomy_runtime_policy::acquire_autonomy_project_authority_lock("PUB")
				.expect("contending authority lock should eventually acquire");

			acquired_tx.send(()).expect("acquired signal should send");

			drop(guard);
		});

		attempting_rx.recv().expect("contender should start");

		assert!(matches!(
			acquired_rx.recv_timeout(Duration::from_millis(100)),
			Err(mpsc::RecvTimeoutError::Timeout)
		));

		drop(outer);

		acquired_rx
			.recv_timeout(Duration::from_secs(2))
			.expect("contender should acquire after release");
		contender.join().expect("contender should finish");
	}
}
