//! Versioned read-only autonomy signal evidence.

mod fingerprint;

mod model;

mod review;

mod types;

mod validation;

#[allow(unused_imports)] pub(crate) use model::AUTONOMY_SIGNAL_SCHEMA;
pub(crate) use model::{AutonomySignal, AutonomySignalInput};

pub(crate) use review::AutonomySignalReviewEvidence;
#[allow(unused_imports)] pub(crate) use review::AutonomySignalReviewRoute;

pub(crate) use types::{
	AutonomySignalConfidence, AutonomySignalEvidenceClass, AutonomySignalFreshness,
	AutonomySignalKind, AutonomySignalPrivacy, AutonomySignalSourceType,
};

#[cfg(test)] mod tests;
