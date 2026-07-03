//! Autonomy signal payload and validation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
	autonomy_signal::{
		fingerprint::{self},
		review::AutonomySignalReviewEvidence,
		types::{
			AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
			AutonomySignalKind, AutonomySignalPrivacy, AutonomySignalSourceType,
		},
		validation::{self},
	},
	prelude::{Result, eyre},
};

pub(crate) const AUTONOMY_SIGNAL_SCHEMA: &str = "decodex.autonomy_signal/1";

const AUTONOMY_SIGNAL_RECORD_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomySignalInput {
	pub(crate) project_id: String,
	pub(crate) objective_id: String,
	pub(crate) objective_version: u64,
	pub(crate) source_type: AutonomySignalSourceType,
	pub(crate) source_refs: Vec<String>,
	pub(crate) primary_source_refs: Vec<String>,
	pub(crate) issue_id: Option<String>,
	pub(crate) run_id: Option<String>,
	pub(crate) attempt_id: Option<String>,
	pub(crate) head_sha: Option<String>,
	pub(crate) captured_at: String,
	pub(crate) freshness: AutonomySignalFreshness,
	pub(crate) summary: String,
	pub(crate) evidence: Vec<String>,
	pub(crate) evidence_class: AutonomySignalEvidenceClass,
	pub(crate) contradictions: Vec<String>,
	pub(crate) gaps: Vec<String>,
	pub(crate) confidence: AutonomySignalConfidence,
	pub(crate) privacy: AutonomySignalPrivacy,
	pub(crate) observed_counts: BTreeMap<String, u64>,
	pub(crate) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(crate) proposal_only: bool,
	pub(crate) created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AutonomySignal {
	#[serde(default = "autonomy_signal_schema")]
	pub(super) schema: String,
	#[serde(default = "autonomy_signal_record_version")]
	pub(super) record_version: u16,
	pub(super) id: String,
	pub(super) fingerprint: String,
	pub(super) project_id: String,
	pub(super) objective_id: String,
	pub(super) objective_version: u64,
	pub(super) kind: AutonomySignalKind,
	pub(super) source_type: AutonomySignalSourceType,
	pub(super) source_refs: Vec<String>,
	#[serde(default)]
	pub(super) primary_source_refs: Vec<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) issue_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) run_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) attempt_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) head_sha: Option<String>,
	pub(super) captured_at: String,
	pub(super) freshness: AutonomySignalFreshness,
	pub(super) summary: String,
	pub(super) evidence: Vec<String>,
	pub(super) evidence_class: AutonomySignalEvidenceClass,
	#[serde(default)]
	pub(super) contradictions: Vec<String>,
	#[serde(default)]
	pub(super) gaps: Vec<String>,
	pub(super) confidence: AutonomySignalConfidence,
	pub(super) privacy: AutonomySignalPrivacy,
	#[serde(default)]
	pub(super) observed_counts: BTreeMap<String, u64>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub(super) review_evidence: Option<AutonomySignalReviewEvidence>,
	pub(super) proposal_only: bool,
	pub(super) created_at: String,
}
#[allow(dead_code)]
impl AutonomySignal {
	pub(crate) fn runtime_health(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::RuntimeHealth, input)
	}

	pub(crate) fn validation_regression(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ValidationRegression, input)
	}

	pub(crate) fn review_feedback_cluster(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ReviewFeedbackCluster, input)
	}

	pub(crate) fn user_feedback_cluster(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::UserFeedbackCluster, input)
	}

	pub(crate) fn spec_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::SpecDrift, input)
	}

	pub(crate) fn protocol_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ProtocolDrift, input)
	}

	#[allow(dead_code)]
	pub(crate) fn metric_regression(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::MetricRegression, input)
	}

	pub(crate) fn execution_friction(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::ExecutionFriction, input)
	}

	pub(crate) fn docs_skill_drift(input: AutonomySignalInput) -> Result<Self> {
		Self::from_input(AutonomySignalKind::DocsSkillDrift, input)
	}

	pub(crate) fn id(&self) -> &str {
		&self.id
	}

	pub(crate) fn fingerprint(&self) -> &str {
		&self.fingerprint
	}

	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn objective_id(&self) -> &str {
		&self.objective_id
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.objective_version
	}

	pub(crate) fn kind(&self) -> AutonomySignalKind {
		self.kind
	}

	pub(crate) fn source_type(&self) -> AutonomySignalSourceType {
		self.source_type
	}

	pub(crate) fn freshness(&self) -> AutonomySignalFreshness {
		self.freshness
	}

	pub(crate) fn evidence_class(&self) -> AutonomySignalEvidenceClass {
		self.evidence_class
	}

	pub(crate) fn confidence(&self) -> AutonomySignalConfidence {
		self.confidence
	}

	pub(crate) fn privacy(&self) -> AutonomySignalPrivacy {
		self.privacy
	}

	pub(crate) fn summary(&self) -> &str {
		&self.summary
	}

	pub(crate) fn gaps(&self) -> &[String] {
		&self.gaps
	}

	pub(crate) fn contradictions(&self) -> &[String] {
		&self.contradictions
	}

	pub(crate) fn source_refs(&self) -> &[String] {
		&self.source_refs
	}

	pub(crate) fn primary_source_refs(&self) -> &[String] {
		&self.primary_source_refs
	}

	pub(crate) fn head_sha(&self) -> Option<&str> {
		self.head_sha.as_deref()
	}

	pub(crate) fn review_evidence(&self) -> Option<&AutonomySignalReviewEvidence> {
		self.review_evidence.as_ref()
	}

	pub(crate) fn validate(&self) -> Result<()> {
		validation::validate_required("autonomy signal schema", &self.schema)?;
		validation::validate_required("autonomy signal id", &self.id)?;
		validation::validate_required("autonomy signal fingerprint", &self.fingerprint)?;
		validation::validate_required("autonomy signal project_id", &self.project_id)?;
		validation::validate_required("autonomy signal objective_id", &self.objective_id)?;
		validation::validate_required("autonomy signal captured_at", &self.captured_at)?;
		validation::validate_required("autonomy signal summary", &self.summary)?;
		validation::validate_required("autonomy signal created_at", &self.created_at)?;
		validation::validate_nonempty_list("autonomy signal source_refs", &self.source_refs)?;
		validation::validate_nonempty_list("autonomy signal evidence", &self.evidence)?;
		validation::validate_string_list(
			"autonomy signal primary_source_refs",
			&self.primary_source_refs,
		)?;
		validation::validate_string_list("autonomy signal contradictions", &self.contradictions)?;
		validation::validate_string_list("autonomy signal gaps", &self.gaps)?;
		validation::validate_optional_required(
			"autonomy signal issue_id",
			self.issue_id.as_deref(),
		)?;
		validation::validate_optional_required("autonomy signal run_id", self.run_id.as_deref())?;
		validation::validate_optional_required(
			"autonomy signal attempt_id",
			self.attempt_id.as_deref(),
		)?;
		validation::validate_optional_required(
			"autonomy signal head_sha",
			self.head_sha.as_deref(),
		)?;

		if self.schema != AUTONOMY_SIGNAL_SCHEMA {
			eyre::bail!("Autonomy signal `{}` has unsupported schema `{}`.", self.id, self.schema);
		}
		if self.record_version != AUTONOMY_SIGNAL_RECORD_VERSION {
			eyre::bail!(
				"Autonomy signal `{}` has unsupported record_version `{}`.",
				self.id,
				self.record_version
			);
		}
		if self.objective_version == 0 {
			eyre::bail!(
				"Autonomy signal `{}` objective_version must be greater than zero.",
				self.id
			);
		}
		if !self.proposal_only {
			eyre::bail!("Autonomy signal `{}` must remain proposal-only evidence.", self.id);
		}

		self.validate_source_specific_rules()?;

		let expected = fingerprint::autonomy_signal_fingerprint(self)?;

		if expected != self.fingerprint {
			eyre::bail!(
				"Autonomy signal `{}` fingerprint mismatch: expected `{expected}`.",
				self.id
			);
		}

		let expected_id = fingerprint::autonomy_signal_id(&expected);

		if expected_id != self.id {
			eyre::bail!(
				"Autonomy signal id `{}` does not match fingerprint `{expected}`.",
				self.id
			);
		}

		Ok(())
	}

	fn from_input(kind: AutonomySignalKind, input: AutonomySignalInput) -> Result<Self> {
		let mut signal = Self {
			schema: autonomy_signal_schema(),
			record_version: autonomy_signal_record_version(),
			id: String::new(),
			fingerprint: String::new(),
			project_id: input.project_id,
			objective_id: input.objective_id,
			objective_version: input.objective_version,
			kind,
			source_type: input.source_type,
			source_refs: input.source_refs,
			primary_source_refs: input.primary_source_refs,
			issue_id: input.issue_id,
			run_id: input.run_id,
			attempt_id: input.attempt_id,
			head_sha: input.head_sha,
			captured_at: input.captured_at,
			freshness: input.freshness,
			summary: input.summary,
			evidence: input.evidence,
			evidence_class: input.evidence_class,
			contradictions: input.contradictions,
			gaps: input.gaps,
			confidence: input.confidence,
			privacy: input.privacy,
			observed_counts: input.observed_counts,
			review_evidence: input.review_evidence,
			proposal_only: input.proposal_only,
			created_at: input.created_at,
		};
		let fingerprint = fingerprint::autonomy_signal_fingerprint(&signal)?;

		signal.id = fingerprint::autonomy_signal_id(&fingerprint);
		signal.fingerprint = fingerprint;

		signal.validate()?;

		Ok(signal)
	}

	fn validate_source_specific_rules(&self) -> Result<()> {
		if matches!(
			self.source_type,
			AutonomySignalSourceType::Memory | AutonomySignalSourceType::Report
		) && self.primary_source_refs.is_empty()
		{
			eyre::bail!(
				"Memory/report autonomy signal `{}` requires primary_source_refs.",
				self.id
			);
		}
		if self.kind == AutonomySignalKind::ReviewFeedbackCluster
			|| self.source_type == AutonomySignalSourceType::Review
		{
			let Some(review_evidence) = &self.review_evidence else {
				eyre::bail!(
					"Review-derived autonomy signal `{}` requires review_evidence.",
					self.id
				);
			};

			review_evidence.validate()?;

			let Some(head_sha) = self.head_sha.as_deref() else {
				eyre::bail!("Review-derived autonomy signal `{}` requires head_sha.", self.id);
			};

			if review_evidence.head_sha != head_sha {
				eyre::bail!(
					"Review-derived autonomy signal `{}` head_sha must match review evidence head.",
					self.id
				);
			}
		}

		Ok(())
	}
}

fn autonomy_signal_schema() -> String {
	AUTONOMY_SIGNAL_SCHEMA.to_owned()
}

const fn autonomy_signal_record_version() -> u16 {
	AUTONOMY_SIGNAL_RECORD_VERSION
}
