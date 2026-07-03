//! Release delta artifact generation and release-window backfill orchestration.

mod backfill;
mod build;
mod comparison;
mod options;
mod selection;

pub(crate) use self::{backfill::backfill_release_range, build::refresh_release_delta};

use serde_json::Value;

#[derive(Clone, Debug)]
pub(super) struct ReleasePair {
	stable: Value,
	preview: Value,
}
