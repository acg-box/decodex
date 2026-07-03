//! Versioned read-only autonomy signal evidence.

mod fingerprint;
mod model;
mod review;
mod types;
mod validation;

#[allow(unused_imports)]
pub(crate) use self::{model::AUTONOMY_SIGNAL_SCHEMA, review::AutonomySignalReviewRoute};
pub(crate) use self::{
	model::{AutonomySignal, AutonomySignalInput},
	review::AutonomySignalReviewEvidence,
};
pub(crate) use types::{
	AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
	AutonomySignalKind, AutonomySignalPrivacy, AutonomySignalSourceType,
};

#[cfg(test)] mod tests;
