use super::{Result, StateStore, eyre, json, state};
use state::PrivateExecutionEvent;

pub(crate) const AUTHORITY_DECISION_REQUEST_SCHEMA: &str = "decodex.authority_decision_request/1";
pub(crate) const AUTHORITY_DECISION_REQUEST_EVENT_TYPE: &str = "authority_decision_request";
#[allow(dead_code)]
pub(crate) const AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE: &str = "authority_boundary_check";
pub(crate) const ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE: &str = "architecture_recovery_packet";
pub(crate) const ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE: &str = "architecture_recovery_started";
pub(crate) const ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE: &str = "architecture_recovery_terminal";
pub(crate) const PHASE_GOAL_RECOVERY_EVENT_TYPE: &str = "phase_goal_recovery";
pub(crate) const PHASE_GOAL_RECOVERY_BLOCKED_EVENT_TYPE: &str = "phase_goal_recovery_blocked";
pub(crate) const PHASE_GOAL_RECOVERY_AUTOMATIC_CONTINUATION_LIMIT: i64 = 1;
pub(crate) const PHASE_ACCEPTANCE_CHECK_EVENT_TYPE: &str = "phase_acceptance_check";

#[allow(dead_code)]
pub(crate) const AUTHORITY_BOUNDARY_CHECK_SCHEMA: &str = "decodex.authority_boundary_check/1";
pub(crate) const ARCHITECTURE_RECOVERY_PACKET_SCHEMA: &str =
	"decodex.architecture_recovery_packet/1";

/// Final authority disposition for one loop recovery boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityBoundaryDisposition {
	WithinAuthority,
	RequiresHuman,
	InsufficientEvidence,
}
impl AuthorityBoundaryDisposition {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::WithinAuthority => "within_authority",
			Self::RequiresHuman => "requires_human",
			Self::InsufficientEvidence => "insufficient_evidence",
		}
	}
}

/// Typed surface considered by an authority boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityBoundarySurface {
	ImplementationStrategy,
	Runtime,
	Tests,
	Docs,
	PublicApi,
	Config,
	Security,
	Data,
	Billing,
	Privacy,
	Validation,
	ReviewPolicy,
	Objective,
	NonGoal,
	ExternalDependency,
	RetainedOwnership,
	AuthorityEvidence,
}
impl AuthorityBoundarySurface {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::ImplementationStrategy => "implementation_strategy",
			Self::Runtime => "runtime",
			Self::Tests => "tests",
			Self::Docs => "docs",
			Self::PublicApi => "public_api",
			Self::Config => "config",
			Self::Security => "security",
			Self::Data => "data",
			Self::Billing => "billing",
			Self::Privacy => "privacy",
			Self::Validation => "validation",
			Self::ReviewPolicy => "review_policy",
			Self::Objective => "objective",
			Self::NonGoal => "non_goal",
			Self::ExternalDependency => "external_dependency",
			Self::RetainedOwnership => "retained_ownership",
			Self::AuthorityEvidence => "authority_evidence",
		}
	}

	pub(crate) fn policy_decision(self) -> AuthorityBoundaryPolicyDecision {
		match self {
			Self::ImplementationStrategy | Self::Runtime | Self::Tests | Self::Docs =>
				AuthorityBoundaryPolicyDecision::AutoContinue,
			Self::PublicApi
			| Self::Config
			| Self::Security
			| Self::Data
			| Self::Billing
			| Self::Privacy => AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
			Self::Validation | Self::ReviewPolicy => AuthorityBoundaryPolicyDecision::BlockLanding,
			Self::Objective
			| Self::NonGoal
			| Self::ExternalDependency
			| Self::RetainedOwnership
			| Self::AuthorityEvidence => AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
		}
	}
}

/// Automation policy decision derived from the changed authority surfaces.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AuthorityBoundaryPolicyDecision {
	AutoContinue,
	RequiresEnhancedEvidence,
	BlockLanding,
	RequiresHumanDecision,
}
impl AuthorityBoundaryPolicyDecision {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::AutoContinue => "auto_continue",
			Self::RequiresEnhancedEvidence => "requires_enhanced_evidence",
			Self::BlockLanding => "block_landing",
			Self::RequiresHumanDecision => "requires_human_decision",
		}
	}

	pub(crate) fn disposition(self) -> AuthorityBoundaryDisposition {
		match self {
			Self::AutoContinue | Self::RequiresEnhancedEvidence | Self::BlockLanding =>
				AuthorityBoundaryDisposition::WithinAuthority,
			Self::RequiresHumanDecision => AuthorityBoundaryDisposition::RequiresHuman,
		}
	}

	pub(crate) fn allows_autonomous_recovery(self) -> bool {
		self != Self::RequiresHumanDecision
	}

	pub(crate) fn requires_enhanced_evidence(self) -> bool {
		matches!(self, Self::RequiresEnhancedEvidence | Self::BlockLanding)
	}

	pub(crate) fn blocks_landing(self) -> bool {
		self == Self::BlockLanding
	}

	pub(crate) fn rank(self) -> u8 {
		match self {
			Self::AutoContinue => 0,
			Self::RequiresEnhancedEvidence => 1,
			Self::BlockLanding => 2,
			Self::RequiresHumanDecision => 3,
		}
	}

	pub(crate) fn max(left: Self, right: Self) -> Self {
		if left.rank() >= right.rank() { left } else { right }
	}
}

/// One surface considered by an authority boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryChangedSurface<'a> {
	pub(crate) surface: AuthorityBoundarySurface,
	pub(crate) change_summary: &'a str,
	pub(crate) policy_decision: AuthorityBoundaryPolicyDecision,
	pub(crate) legacy_disposition: AuthorityBoundaryDisposition,
}

/// Sanitized harness feedback emitted from an authority boundary check.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryImprovementSignal<'a> {
	pub(crate) kind: &'a str,
	pub(crate) reason_code: &'a str,
	pub(crate) target: &'a str,
	pub(crate) recommendation: &'a str,
}

/// Input for persisting a structured authority boundary check as private evidence.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityBoundaryCheckInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) decision_contract_ids: Vec<&'a str>,
	pub(crate) attempted_recovery_reason: &'a str,
	pub(crate) changed_surfaces: Vec<AuthorityBoundaryChangedSurface<'a>>,
	pub(crate) policy_decision: AuthorityBoundaryPolicyDecision,
	pub(crate) disposition: AuthorityBoundaryDisposition,
	pub(crate) final_disposition_reason: &'a str,
	pub(crate) improvement_signals: Vec<AuthorityBoundaryImprovementSignal<'a>>,
}

/// One public-safe option offered in a durable authority decision request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionOption<'a> {
	pub(crate) label: &'a str,
	pub(crate) description: &'a str,
}

/// Input for persisting the full local decision packet for an authority-boundary stop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorityDecisionRequestInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) boundary_check_record_id: i64,
	pub(crate) decision_request_id: &'a str,
	pub(crate) reason_code: &'a str,
	pub(crate) boundary_type: &'a str,
	pub(crate) proposed_change: &'a str,
	pub(crate) why_exceeds_authority: &'a str,
	pub(crate) options: Vec<AuthorityDecisionOption<'a>>,
	pub(crate) recommendation: &'a str,
	pub(crate) resume_condition: &'a str,
	pub(crate) retained_worktree_evidence: Vec<&'a str>,
	pub(crate) retained_diff_evidence: Vec<&'a str>,
	pub(crate) recovery_attempt_context: Vec<&'a str>,
}

#[allow(dead_code)]
pub(crate) fn record_authority_boundary_check_private_event(
	state_store: &StateStore,
	input: AuthorityBoundaryCheckInput<'_>,
) -> Result<PrivateExecutionEvent> {
	validate_authority_boundary_check_input(&input)?;

	let changed_surfaces = input
		.changed_surfaces
		.iter()
		.map(|surface| {
			json!({
				"surface": surface.surface.as_str(),
				"change_summary": surface.change_summary,
				"policy_decision": surface.policy_decision.as_str(),
				"legacy_disposition": surface.legacy_disposition.as_str(),
			})
		})
		.collect::<Vec<_>>();
	let improvement_signals = input
		.improvement_signals
		.iter()
		.map(|signal| {
			json!({
				"kind": signal.kind,
				"reason_code": signal.reason_code,
				"target": signal.target,
				"recommendation": signal.recommendation,
			})
		})
		.collect::<Vec<_>>();
	let payload = json!({
		"schema": AUTHORITY_BOUNDARY_CHECK_SCHEMA,
		"record_version": 1,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"decision_contract_ids": input.decision_contract_ids,
		"attempted_recovery_reason": input.attempted_recovery_reason,
		"changed_surfaces": changed_surfaces,
		"policy_decision": input.policy_decision.as_str(),
		"policy": {
			"decision": input.policy_decision.as_str(),
			"allows_autonomous_recovery": input.policy_decision.allows_autonomous_recovery(),
			"requires_enhanced_evidence": input.policy_decision.requires_enhanced_evidence(),
			"blocks_landing": input.policy_decision.blocks_landing(),
		},
		"disposition": input.disposition.as_str(),
		"final_disposition": {
			"disposition": input.disposition.as_str(),
			"reason": input.final_disposition_reason,
		},
		"improvement_signals": improvement_signals,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		payload,
	)
}

pub(crate) fn record_authority_decision_request_private_event(
	state_store: &StateStore,
	input: AuthorityDecisionRequestInput<'_>,
) -> Result<PrivateExecutionEvent> {
	validate_authority_decision_request_input(&input)?;

	let options = input
		.options
		.iter()
		.map(|option| {
			json!({
				"label": option.label,
				"description": option.description,
			})
		})
		.collect::<Vec<_>>();
	let payload = json!({
		"schema": AUTHORITY_DECISION_REQUEST_SCHEMA,
		"record_version": 1,
		"decision_request_id": input.decision_request_id,
		"issue": {
			"id": input.issue_id,
			"identifier": input.issue_identifier,
		},
		"run": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number,
		},
		"authority_boundary_check": {
			"record_id": input.boundary_check_record_id,
			"event_type": AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
		},
		"phase": "human_required",
		"reason": input.reason_code,
		"boundary": input.boundary_type,
		"proposed_change": input.proposed_change,
		"why_exceeds_authority": input.why_exceeds_authority,
		"options": options,
		"recommendation": input.recommendation,
		"resume_condition": input.resume_condition,
		"next_action": input.resume_condition,
		"retained_worktree_evidence": input.retained_worktree_evidence,
		"retained_diff_evidence": input.retained_diff_evidence,
		"recovery_attempt_context": input.recovery_attempt_context,
	});

	state_store.append_private_execution_event(
		input.project_id,
		input.issue_id,
		input.run_id,
		input.attempt_number,
		AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		payload,
	)
}

pub(crate) fn validate_authority_boundary_check_input(
	input: &AuthorityBoundaryCheckInput<'_>,
) -> Result<()> {
	authority_boundary_required("authority boundary project_id", input.project_id)?;
	authority_boundary_required("authority boundary issue_id", input.issue_id)?;
	authority_boundary_required("authority boundary issue_identifier", input.issue_identifier)?;
	authority_boundary_required("authority boundary run_id", input.run_id)?;
	authority_boundary_required(
		"authority boundary attempted_recovery_reason",
		input.attempted_recovery_reason,
	)?;
	authority_boundary_required(
		"authority boundary final_disposition_reason",
		input.final_disposition_reason,
	)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority boundary attempt_number must be positive.");
	}
	if input.changed_surfaces.is_empty() {
		eyre::bail!("Authority boundary changed_surfaces must not be empty.");
	}

	let mut expected_policy_decision = AuthorityBoundaryPolicyDecision::AutoContinue;

	for surface in &input.changed_surfaces {
		let surface_policy_decision = surface.surface.policy_decision();

		if surface.policy_decision != surface_policy_decision {
			eyre::bail!(
				"Authority boundary surface `{}` must use policy decision `{}`.",
				surface.surface.as_str(),
				surface_policy_decision.as_str()
			);
		}
		if surface.legacy_disposition != surface_policy_decision.disposition() {
			eyre::bail!(
				"Authority boundary surface `{}` must use legacy disposition `{}`.",
				surface.surface.as_str(),
				surface_policy_decision.disposition().as_str()
			);
		}

		expected_policy_decision =
			AuthorityBoundaryPolicyDecision::max(expected_policy_decision, surface_policy_decision);
	}

	if input.policy_decision != expected_policy_decision {
		eyre::bail!(
			"Authority boundary policy_decision must be `{}` for the changed surfaces.",
			expected_policy_decision.as_str()
		);
	}
	if input.disposition != input.policy_decision.disposition() {
		eyre::bail!(
			"Authority boundary disposition must be `{}` for policy decision `{}`.",
			input.policy_decision.disposition().as_str(),
			input.policy_decision.as_str()
		);
	}

	for contract_id in &input.decision_contract_ids {
		authority_boundary_required("authority boundary decision_contract_id", contract_id)?;
	}
	for surface in &input.changed_surfaces {
		authority_boundary_required(
			"authority boundary changed surface summary",
			surface.change_summary,
		)?;
	}
	for signal in &input.improvement_signals {
		authority_boundary_required("authority boundary improvement kind", signal.kind)?;
		authority_boundary_required(
			"authority boundary improvement reason_code",
			signal.reason_code,
		)?;
		authority_boundary_required("authority boundary improvement target", signal.target)?;
		authority_boundary_required(
			"authority boundary improvement recommendation",
			signal.recommendation,
		)?;
	}

	Ok(())
}

pub(crate) fn validate_authority_decision_request_input(
	input: &AuthorityDecisionRequestInput<'_>,
) -> Result<()> {
	authority_boundary_required("authority decision project_id", input.project_id)?;
	authority_boundary_required("authority decision issue_id", input.issue_id)?;
	authority_boundary_required("authority decision issue_identifier", input.issue_identifier)?;
	authority_boundary_required("authority decision run_id", input.run_id)?;
	authority_boundary_required(
		"authority decision decision_request_id",
		input.decision_request_id,
	)?;
	authority_boundary_required("authority decision reason_code", input.reason_code)?;
	authority_boundary_required("authority decision boundary_type", input.boundary_type)?;
	authority_boundary_required("authority decision proposed_change", input.proposed_change)?;
	authority_boundary_required(
		"authority decision why_exceeds_authority",
		input.why_exceeds_authority,
	)?;
	authority_boundary_required("authority decision recommendation", input.recommendation)?;
	authority_boundary_required("authority decision resume_condition", input.resume_condition)?;

	if input.attempt_number < 1 {
		eyre::bail!("Authority decision attempt_number must be positive.");
	}
	if input.boundary_check_record_id < 1 {
		eyre::bail!("Authority decision boundary_check_record_id must be positive.");
	}
	if input.options.is_empty() {
		eyre::bail!("Authority decision options must not be empty.");
	}

	for option in &input.options {
		authority_boundary_required("authority decision option label", option.label)?;
		authority_boundary_required("authority decision option description", option.description)?;
	}
	for evidence in &input.retained_worktree_evidence {
		authority_boundary_required("authority decision retained_worktree_evidence", evidence)?;
	}
	for evidence in &input.retained_diff_evidence {
		authority_boundary_required("authority decision retained_diff_evidence", evidence)?;
	}
	for context in &input.recovery_attempt_context {
		authority_boundary_required("authority decision recovery_attempt_context", context)?;
	}

	Ok(())
}

pub(crate) fn authority_boundary_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}
