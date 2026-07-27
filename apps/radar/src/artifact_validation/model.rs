//! Shared validation result and state types.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::PathBuf,
};

pub(crate) struct ArtifactValidation {
	pub(crate) schema: Option<String>,
	pub(crate) errors: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct ValidationState {
	pub(super) seen_signal_slugs: BTreeMap<String, PathBuf>,
}
impl ValidationState {
	pub(crate) fn new() -> Self {
		Self { seen_signal_slugs: BTreeMap::new() }
	}
}

#[derive(Debug, Default)]
pub(super) struct ReleaseOptionTags {
	pub(super) stable: BTreeSet<String>,
	pub(super) preview: BTreeSet<String>,
}
