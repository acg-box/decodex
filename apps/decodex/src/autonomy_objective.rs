//! Versioned Objective Contract model for project-autonomy authority.

use serde::{Deserialize, Serialize};

use crate::prelude::{Result, eyre};

pub(crate) const AUTONOMY_OBJECTIVE_SCHEMA: &str = "decodex.autonomy_objective/1";
pub(crate) const AUTONOMY_OBJECTIVE_RECORD_VERSION: u16 = 1;

/// Runtime-facing lifecycle state for an Objective Contract version.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyObjectiveState {
	Draft,
	Accepted,
	Rejected,
	Superseded,
}
impl AutonomyObjectiveState {
	pub(crate) fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::Accepted => "accepted",
			Self::Rejected => "rejected",
			Self::Superseded => "superseded",
		}
	}
}

/// Actor class for objective lifecycle changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AutonomyObjectiveActorKind {
	User,
	RuntimePolicy,
}

/// Acceptance metadata that turns a draft objective version into authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveAcceptance {
	accepted_by: String,
	accepted_by_kind: AutonomyObjectiveActorKind,
	accepted_at: String,
	acceptance_source: String,
}
#[allow(dead_code)]
impl AutonomyObjectiveAcceptance {
	pub(crate) fn new(
		accepted_by: impl Into<String>,
		accepted_by_kind: AutonomyObjectiveActorKind,
		accepted_at: impl Into<String>,
		acceptance_source: impl Into<String>,
	) -> Result<Self> {
		let acceptance = Self {
			accepted_by: accepted_by.into(),
			accepted_by_kind,
			accepted_at: accepted_at.into(),
			acceptance_source: acceptance_source.into(),
		};

		acceptance.validate()?;

		Ok(acceptance)
	}

	pub(crate) fn accepted_by(&self) -> &str {
		&self.accepted_by
	}

	pub(crate) fn accepted_at(&self) -> &str {
		&self.accepted_at
	}

	pub(crate) fn acceptance_source(&self) -> &str {
		&self.acceptance_source
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy objective acceptance.accepted_by", &self.accepted_by)?;
		validate_required("autonomy objective acceptance.accepted_at", &self.accepted_at)?;

		validate_required(
			"autonomy objective acceptance.acceptance_source",
			&self.acceptance_source,
		)
	}
}

/// Rejection metadata for a draft objective version that did not become authority.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveRejection {
	rejected_by: String,
	rejected_at: String,
	rejection_source: String,
	reason: String,
}
#[allow(dead_code)]
impl AutonomyObjectiveRejection {
	pub(crate) fn new(
		rejected_by: impl Into<String>,
		rejected_at: impl Into<String>,
		rejection_source: impl Into<String>,
		reason: impl Into<String>,
	) -> Result<Self> {
		let rejection = Self {
			rejected_by: rejected_by.into(),
			rejected_at: rejected_at.into(),
			rejection_source: rejection_source.into(),
			reason: reason.into(),
		};

		rejection.validate()?;

		Ok(rejection)
	}

	pub(crate) fn reason(&self) -> &str {
		&self.reason
	}

	fn validate(&self) -> Result<()> {
		validate_required("autonomy objective rejection.rejected_by", &self.rejected_by)?;
		validate_required("autonomy objective rejection.rejected_at", &self.rejected_at)?;
		validate_required("autonomy objective rejection.rejection_source", &self.rejection_source)?;

		validate_required("autonomy objective rejection.reason", &self.reason)
	}
}

/// Supersession metadata linking an older objective version to the replacing version.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveSupersession {
	superseded_by_objective_id: String,
	superseded_by_version: u64,
	superseded_by: String,
	superseded_at: String,
	supersession_source: String,
	reason: String,
}
#[allow(dead_code)]
impl AutonomyObjectiveSupersession {
	pub(crate) fn new(
		superseded_by_objective_id: impl Into<String>,
		superseded_by_version: u64,
		superseded_by: impl Into<String>,
		superseded_at: impl Into<String>,
		supersession_source: impl Into<String>,
		reason: impl Into<String>,
	) -> Result<Self> {
		let supersession = Self {
			superseded_by_objective_id: superseded_by_objective_id.into(),
			superseded_by_version,
			superseded_by: superseded_by.into(),
			superseded_at: superseded_at.into(),
			supersession_source: supersession_source.into(),
			reason: reason.into(),
		};

		supersession.validate()?;

		Ok(supersession)
	}

	pub(crate) fn superseded_by_objective_id(&self) -> &str {
		&self.superseded_by_objective_id
	}

	pub(crate) fn superseded_by_version(&self) -> u64 {
		self.superseded_by_version
	}

	fn validate(&self) -> Result<()> {
		validate_required(
			"autonomy objective supersession.superseded_by_objective_id",
			&self.superseded_by_objective_id,
		)?;

		if self.superseded_by_version == 0 {
			eyre::bail!(
				"Autonomy objective supersession.superseded_by_version must be greater than zero."
			);
		}

		validate_required("autonomy objective supersession.superseded_by", &self.superseded_by)?;
		validate_required("autonomy objective supersession.superseded_at", &self.superseded_at)?;
		validate_required(
			"autonomy objective supersession.supersession_source",
			&self.supersession_source,
		)?;

		validate_required("autonomy objective supersession.reason", &self.reason)
	}
}

/// Versioned project-level Objective Contract payload.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomyObjectiveContract {
	#[serde(default = "autonomy_objective_schema")]
	schema: String,
	#[serde(default = "autonomy_objective_record_version")]
	record_version: u16,
	project_id: String,
	id: String,
	version: u64,
	state: AutonomyObjectiveState,
	summary: String,
	#[serde(default)]
	goals: Vec<String>,
	#[serde(default)]
	non_goals: Vec<String>,
	#[serde(default)]
	metrics: Vec<String>,
	#[serde(default)]
	allowed_surfaces: Vec<String>,
	#[serde(default)]
	allowed_signal_kinds: Vec<String>,
	#[serde(default)]
	validation_gates: Vec<String>,
	review_policy: String,
	memory_policy: String,
	report_policy: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	acceptance: Option<AutonomyObjectiveAcceptance>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rejection: Option<AutonomyObjectiveRejection>,
	#[serde(skip_serializing_if = "Option::is_none")]
	supersession: Option<AutonomyObjectiveSupersession>,
}
#[allow(dead_code)]
impl AutonomyObjectiveContract {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn version(&self) -> u64 {
		self.version
	}

	pub(crate) fn state(&self) -> AutonomyObjectiveState {
		self.state
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn goals(&self) -> &[String] {
		&self.goals
	}

	pub(crate) fn non_goals(&self) -> &[String] {
		&self.non_goals
	}

	pub(crate) fn metrics(&self) -> &[String] {
		&self.metrics
	}

	pub(crate) fn allowed_surfaces(&self) -> &[String] {
		&self.allowed_surfaces
	}

	pub(crate) fn allowed_signal_kinds(&self) -> &[String] {
		&self.allowed_signal_kinds
	}

	pub(crate) fn validation_gates(&self) -> &[String] {
		&self.validation_gates
	}

	pub(crate) fn review_policy(&self) -> &str {
		&self.review_policy
	}

	pub(crate) fn acceptance(&self) -> Option<&AutonomyObjectiveAcceptance> {
		self.acceptance.as_ref()
	}

	pub(crate) fn rejection(&self) -> Option<&AutonomyObjectiveRejection> {
		self.rejection.as_ref()
	}

	pub(crate) fn supersession(&self) -> Option<&AutonomyObjectiveSupersession> {
		self.supersession.as_ref()
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validate_required("autonomy objective schema", &self.schema)?;
		validate_required("autonomy objective project_id", &self.project_id)?;
		validate_required("autonomy objective id", &self.id)?;
		validate_required("autonomy objective summary", &self.summary)?;
		validate_required("autonomy objective review_policy", &self.review_policy)?;
		validate_required("autonomy objective memory_policy", &self.memory_policy)?;
		validate_required("autonomy objective report_policy", &self.report_policy)?;

		if self.schema != AUTONOMY_OBJECTIVE_SCHEMA {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported schema `{}`.",
				self.id,
				self.schema
			);
		}
		if self.record_version != AUTONOMY_OBJECTIVE_RECORD_VERSION {
			eyre::bail!(
				"Autonomy objective `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.version == 0 {
			eyre::bail!("Autonomy objective `{}` version must be greater than zero.", self.id);
		}

		validate_string_list("autonomy objective goals", &self.goals)?;
		validate_string_list("autonomy objective non_goals", &self.non_goals)?;
		validate_string_list("autonomy objective metrics", &self.metrics)?;
		validate_string_list("autonomy objective allowed_surfaces", &self.allowed_surfaces)?;
		validate_string_list(
			"autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;
		validate_string_list("autonomy objective validation_gates", &self.validation_gates)?;

		match self.state {
			AutonomyObjectiveState::Draft => {
				if self.acceptance.is_some()
					|| self.rejection.is_some()
					|| self.supersession.is_some()
				{
					eyre::bail!(
						"Draft autonomy objective `{}` must not carry lifecycle provenance.",
						self.id
					);
				}
			},
			AutonomyObjectiveState::Accepted => {
				if self.acceptance.is_none() {
					eyre::bail!(
						"Accepted autonomy objective `{}` must include acceptance.",
						self.id
					);
				}
				if self.rejection.is_some() || self.supersession.is_some() {
					eyre::bail!(
						"Accepted autonomy objective `{}` must not carry rejection or supersession.",
						self.id
					);
				}

				self.validate_complete_authority_body()?;
			},
			AutonomyObjectiveState::Rejected => {
				if self.rejection.is_none() {
					eyre::bail!(
						"Rejected autonomy objective `{}` must include rejection.",
						self.id
					);
				}
				if self.acceptance.is_some() || self.supersession.is_some() {
					eyre::bail!(
						"Rejected autonomy objective `{}` must not carry acceptance or supersession.",
						self.id
					);
				}
			},
			AutonomyObjectiveState::Superseded => {
				if self.supersession.is_none() {
					eyre::bail!(
						"Superseded autonomy objective `{}` must include supersession.",
						self.id
					);
				}
				if self.rejection.is_some() {
					eyre::bail!(
						"Superseded autonomy objective `{}` must not carry rejection.",
						self.id
					);
				}
			},
		}

		if let Some(acceptance) = &self.acceptance {
			acceptance.validate()?;
		}
		if let Some(rejection) = &self.rejection {
			rejection.validate()?;
		}
		if let Some(supersession) = &self.supersession {
			supersession.validate()?;

			if supersession.superseded_by_objective_id() == self.id
				&& supersession.superseded_by_version() <= self.version
			{
				eyre::bail!(
					"Autonomy objective `{}` version {} cannot be superseded by same-objective version {}.",
					self.id,
					self.version,
					supersession.superseded_by_version()
				);
			}
		}

		Ok(())
	}

	pub(crate) fn accept(&mut self, acceptance: AutonomyObjectiveAcceptance) -> Result<()> {
		match self.state {
			AutonomyObjectiveState::Draft => {},
			AutonomyObjectiveState::Accepted => {
				eyre::bail!(
					"Autonomy objective `{}` version {} is already accepted.",
					self.id,
					self.version
				);
			},
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded => {
				eyre::bail!(
					"Autonomy objective `{}` version {} cannot be accepted from state `{}`.",
					self.id,
					self.version,
					self.state.as_str()
				);
			},
		}

		acceptance.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Accepted;
		candidate.acceptance = Some(acceptance);
		candidate.rejection = None;
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn reject(&mut self, rejection: AutonomyObjectiveRejection) -> Result<()> {
		if self.state != AutonomyObjectiveState::Draft {
			eyre::bail!(
				"Autonomy objective `{}` version {} can only be rejected from draft state.",
				self.id,
				self.version
			);
		}

		rejection.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Rejected;
		candidate.acceptance = None;
		candidate.rejection = Some(rejection);
		candidate.supersession = None;

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	pub(crate) fn supersede(&mut self, supersession: AutonomyObjectiveSupersession) -> Result<()> {
		if matches!(
			self.state,
			AutonomyObjectiveState::Rejected | AutonomyObjectiveState::Superseded
		) {
			eyre::bail!(
				"Autonomy objective `{}` version {} cannot be superseded from state `{}`.",
				self.id,
				self.version,
				self.state.as_str()
			);
		}

		supersession.validate()?;

		let mut candidate = self.clone();

		candidate.state = AutonomyObjectiveState::Superseded;
		candidate.rejection = None;
		candidate.supersession = Some(supersession);

		candidate.validate()?;

		*self = candidate;

		Ok(())
	}

	fn validate_complete_authority_body(&self) -> Result<()> {
		validate_nonempty_list("accepted autonomy objective goals", &self.goals)?;
		validate_nonempty_list("accepted autonomy objective non_goals", &self.non_goals)?;
		validate_nonempty_list("accepted autonomy objective metrics", &self.metrics)?;
		validate_nonempty_list(
			"accepted autonomy objective allowed_surfaces",
			&self.allowed_surfaces,
		)?;
		validate_nonempty_list(
			"accepted autonomy objective allowed_signal_kinds",
			&self.allowed_signal_kinds,
		)?;

		validate_nonempty_list(
			"accepted autonomy objective validation_gates",
			&self.validation_gates,
		)
	}
}

fn autonomy_objective_schema() -> String {
	AUTONOMY_OBJECTIVE_SCHEMA.to_owned()
}

const fn autonomy_objective_record_version() -> u16 {
	AUTONOMY_OBJECTIVE_RECORD_VERSION
}

fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

fn validate_nonempty_list(name: &str, values: &[String]) -> Result<()> {
	if values.is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	validate_string_list(name, values)
}

#[cfg(test)]
mod tests {

	use crate::autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
		AutonomyObjectiveRejection, AutonomyObjectiveState, AutonomyObjectiveSupersession,
	};

	fn objective_fixture() -> AutonomyObjectiveContract {
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
			"allowed_signal_kinds": ["validation_regression", "review_feedback_cluster"],
			"validation_gates": ["cargo make check-docs"],
			"review_policy": "independent current-head review required",
			"memory_policy": "read-only source-linked memory only",
			"report_policy": "public-safe summaries only"
		}))
		.expect("objective fixture should parse")
	}

	fn sample_acceptance() -> AutonomyObjectiveAcceptance {
		AutonomyObjectiveAcceptance::new(
			"operator",
			AutonomyObjectiveActorKind::User,
			"2026-06-22T00:00:00Z",
			"conversation",
		)
		.expect("acceptance should validate")
	}

	#[test]
	fn autonomy_objective_acceptance_is_explicit_lifecycle() {
		let mut objective = objective_fixture();

		objective.validate().expect("draft should validate");
		objective.accept(sample_acceptance()).expect("draft should accept");

		assert_eq!(objective.state(), AutonomyObjectiveState::Accepted);
		assert_eq!(
			objective.acceptance().expect("acceptance should exist").accepted_by(),
			"operator"
		);
		assert!(objective.rejection().is_none());
	}

	#[test]
	fn rejected_and_superseded_objectives_keep_provenance() {
		let mut rejected = objective_fixture();

		rejected
			.reject(
				AutonomyObjectiveRejection::new(
					"operator",
					"2026-06-22T00:00:00Z",
					"conversation",
					"Wrong surface.",
				)
				.expect("rejection should validate"),
			)
			.expect("draft should reject");

		assert_eq!(rejected.state(), AutonomyObjectiveState::Rejected);
		assert_eq!(
			rejected.rejection().expect("rejection should exist").reason(),
			"Wrong surface."
		);

		let mut superseded = objective_fixture();

		superseded.accept(sample_acceptance()).expect("draft should accept");
		superseded
			.supersede(
				AutonomyObjectiveSupersession::new(
					"quality-autonomy",
					2,
					"operator",
					"2026-06-22T00:05:00Z",
					"conversation",
					"Accepted replacement objective version.",
				)
				.expect("supersession should validate"),
			)
			.expect("accepted version should supersede");

		assert_eq!(superseded.state(), AutonomyObjectiveState::Superseded);
		assert_eq!(
			superseded.supersession().expect("supersession should exist").superseded_by_version(),
			2
		);
	}

	#[test]
	fn lifecycle_metadata_is_not_inferred_or_accepted_on_drafts() {
		let mut objective =
			serde_json::to_value(objective_fixture()).expect("fixture should encode");

		objective["acceptance"] = serde_json::json!({
			"accepted_by": "operator",
			"accepted_by_kind": "user",
			"accepted_at": "2026-06-22T00:00:00Z",
			"acceptance_source": "conversation"
		});

		let objective =
			serde_json::from_value::<AutonomyObjectiveContract>(objective).expect("payload parses");

		assert!(objective.validate().is_err());
	}

	#[test]
	fn superseded_objectives_reject_mixed_terminal_provenance() {
		let mut objective =
			serde_json::to_value(objective_fixture()).expect("fixture should encode");

		objective["state"] = serde_json::json!("superseded");
		objective["supersession"] = serde_json::json!({
			"superseded_by_objective_id": "quality-autonomy",
			"superseded_by_version": 2,
			"superseded_by": "operator",
			"superseded_at": "2026-06-22T00:05:00Z",
			"supersession_source": "conversation",
			"reason": "Accepted replacement objective version."
		});
		objective["rejection"] = serde_json::json!({
			"rejected_by": "operator",
			"rejected_at": "2026-06-22T00:04:00Z",
			"rejection_source": "conversation",
			"reason": "Contradictory terminal state."
		});

		let objective =
			serde_json::from_value::<AutonomyObjectiveContract>(objective).expect("payload parses");

		assert!(objective.validate().is_err());
	}

	#[test]
	fn superseded_objectives_reject_self_or_older_same_objective_version() {
		for (objective_version, superseded_by_version) in [(1, 1), (2, 1)] {
			let mut objective =
				serde_json::to_value(objective_fixture()).expect("fixture should encode");

			objective["version"] = serde_json::json!(objective_version);
			objective["state"] = serde_json::json!("superseded");
			objective["supersession"] = serde_json::json!({
				"superseded_by_objective_id": "quality-autonomy",
				"superseded_by_version": superseded_by_version,
				"superseded_by": "operator",
				"superseded_at": "2026-06-22T00:05:00Z",
				"supersession_source": "conversation",
				"reason": "Invalid replacement version."
			});

			let objective = serde_json::from_value::<AutonomyObjectiveContract>(objective)
				.expect("payload parses");

			assert!(objective.validate().is_err());
		}
	}
}
