//! Shared validation result and state types.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::PathBuf,
};

pub(in crate::radar) struct ArtifactValidation {
	pub(in crate::radar) schema: Option<String>,
	pub(in crate::radar) errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::radar) struct ArtifactValidationOptions {
	pub(super) allow_historical_archive_retention: bool,
	pub(super) allow_historical_upstream_review_linear_followup: bool,
}

#[derive(Debug)]
pub(in crate::radar) struct ValidationState {
	pub(super) active_social_publish_reservation_idempotency_keys: BTreeMap<String, PathBuf>,
	pub(super) seen_terminal_social_post_idempotency_keys: BTreeMap<String, PathBuf>,
	pub(super) seen_signal_slugs: BTreeMap<String, PathBuf>,
}
impl ValidationState {
	pub(in crate::radar) fn new() -> Self {
		Self {
			active_social_publish_reservation_idempotency_keys: BTreeMap::new(),
			seen_terminal_social_post_idempotency_keys: BTreeMap::new(),
			seen_signal_slugs: BTreeMap::new(),
		}
	}
}

#[derive(Debug, Default)]
pub(super) struct ReleaseOptionTags {
	pub(super) stable: BTreeSet<String>,
	pub(super) preview: BTreeSet<String>,
}
