//! Public entrypoints and dispatch for Radar artifact validation.

mod analysis;
mod dispatch;
mod paths;

pub(crate) use self::{
	analysis::{validate_analysis_draft, validate_signal_file},
	dispatch::{validate_artifact, validate_artifact_errors, validate_artifact_for_path},
};
