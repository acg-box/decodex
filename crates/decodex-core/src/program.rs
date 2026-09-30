//! Identities and state vocabulary for retained Program history.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

macro_rules! stable_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ProgramError> {
				let value = value.into();

				if !is_canonical_uuid_v4(&value) {
					return Err(ProgramError::$error);
				}

				Ok(Self(value))
			}

			#[doc = concat!("Borrow the canonical ", $label, " identity.")]
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
		impl Serialize for $name {
			fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
			where
				S: Serializer,
			{
				serializer.serialize_str(&self.0)
			}
		}
		impl<'de> Deserialize<'de> for $name {
			fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
			where
				D: Deserializer<'de>,
			{
				Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
			}
		}
	};
}

stable_id!(ProgramId, InvalidProgramId, "Program");
stable_id!(ObjectiveId, InvalidObjectiveId, "Objective");
stable_id!(ProgramObservationId, InvalidObservationId, "Program observation");
stable_id!(ProgramClaimId, InvalidClaimId, "Program claim");
stable_id!(ProgramProposalId, InvalidProposalId, "Program proposal");
stable_id!(ProgramEvidenceId, InvalidProgramEvidenceId, "Program evidence");
stable_id!(ProgramReviewId, InvalidReviewId, "Program review");

/// Maximum nodes in one bounded Program causal projection.
pub const MAX_PROGRAM_PROJECTION_NODES: usize = 128;

/// Closed Program-domain validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramError {
	/// Program identity was not canonical UUID-v4 text.
	InvalidProgramId,
	/// Objective identity was not canonical UUID-v4 text.
	InvalidObjectiveId,
	/// Metric, signal, or context-decision identity was not canonical UUID-v4 text.
	InvalidObservationId,
	/// Claim identity was not canonical UUID-v4 text.
	InvalidClaimId,
	/// Proposal identity was not canonical UUID-v4 text.
	InvalidProposalId,
	/// Program Evidence identity was not canonical UUID-v4 text.
	InvalidProgramEvidenceId,
	/// Program Review identity was not canonical UUID-v4 text.
	InvalidReviewId,
}
impl Error for ProgramError {}

impl Display for ProgramError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidProgramId => "invalid Program identity",
			Self::InvalidObjectiveId => "invalid Objective identity",
			Self::InvalidObservationId => "invalid Program observation identity",
			Self::InvalidClaimId => "invalid Program claim identity",
			Self::InvalidProposalId => "invalid Program proposal identity",
			Self::InvalidProgramEvidenceId => "invalid Program evidence identity",
			Self::InvalidReviewId => "invalid Program review identity",
		})
	}
}

/// Open-ended Program lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramState {
	/// Responsibility is operating normally.
	Active,
	/// Responsibility requires Lead review without pretending it is complete.
	NeedsAttention,
	/// Responsibility cannot currently progress.
	Blocked,
	/// Responsibility is intentionally inactive but resumable.
	Paused,
	/// Responsibility is permanently closed and retained for readback.
	Retired,
}
impl ProgramState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Active => "active",
			Self::NeedsAttention => "needs_attention",
			Self::Blocked => "blocked",
			Self::Paused => "paused",
			Self::Retired => "retired",
		}
	}
}

/// Finite Objective lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectiveState {
	/// Outcome is defined but has not begun.
	Proposed,
	/// Work toward the finite outcome is active.
	Active,
	/// Progress toward the outcome is temporarily blocked.
	Blocked,
	/// Immutable acceptance and validation evidence established the outcome.
	Achieved,
	/// Outcome ended intentionally without achievement.
	Abandoned,
}
impl ObjectiveState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Proposed => "proposed",
			Self::Active => "active",
			Self::Blocked => "blocked",
			Self::Achieved => "achieved",
			Self::Abandoned => "abandoned",
		}
	}
}

/// Closed evidence kinds required by the first Program review loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramEvidenceKind {
	/// Reproducible validation with a deterministic command, check, or equivalent witness.
	DeterministicValidation,
	/// Observation from outside the produced artifact or model response.
	External,
}
impl ProgramEvidenceKind {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::DeterministicValidation => "deterministic_validation",
			Self::External => "external",
		}
	}
}

/// Evidence-backed classification recorded by one Program review.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramReviewClassification {
	/// An external or user-visible result improved.
	OutcomeProgress,
	/// Material uncertainty decreased.
	KnowledgeProgress,
	/// A reusable ability or validation mechanism improved.
	CapabilityProgress,
	/// The cycle produced no material delta.
	NoMaterialChange,
	/// Evidence shows that the state became worse.
	Regression,
	/// Evidence is missing, stale, ambiguous, or contradictory.
	Unknown,
}
impl ProgramReviewClassification {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::OutcomeProgress => "outcome_progress",
			Self::KnowledgeProgress => "knowledge_progress",
			Self::CapabilityProgress => "capability_progress",
			Self::NoMaterialChange => "no_material_change",
			Self::Regression => "regression",
			Self::Unknown => "unknown",
		}
	}
}
fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.len() == 36
		&& bytes[8] == b'-'
		&& bytes[13] == b'-'
		&& bytes[18] == b'-'
		&& bytes[23] == b'-'
		&& bytes[14] == b'4'
		&& matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
		&& bytes.iter().enumerate().all(|(index, byte)| {
			matches!(index, 8 | 13 | 18 | 23)
				|| byte.is_ascii_digit()
				|| matches!(byte, b'a'..=b'f')
		})
}
