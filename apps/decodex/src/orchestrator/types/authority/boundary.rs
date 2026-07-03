use crate::orchestrator::types::{Result, eyre};

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

pub(crate) fn authority_boundary_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}
