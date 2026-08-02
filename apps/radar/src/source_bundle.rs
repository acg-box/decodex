//! GitHub source payload normalization for Radar bundle artifacts.

mod builders;
mod evidence;
mod extraction;
mod fields;
mod items;
mod refs;
mod validation;

use crate::{Value, prelude::Result};

#[cfg(test)] pub(crate) use evidence::install_bundle_after_write;
pub(crate) use evidence::{bundle_evidence_from_bytes, install_bundle};

pub(super) fn build_pr_bundle_from_sources(
	repo: &str,
	pr: &Value,
	commits: &[Value],
	files: &[Value],
	default_branch: &str,
	notes: &[String],
) -> Result<Value> {
	builders::build_pr_bundle_from_sources(repo, pr, commits, files, default_branch, notes)
}

pub(super) fn build_commit_bundle_from_sources(
	repo: &str,
	commit: &Value,
	default_branch: &str,
	notes: &[String],
) -> Result<Value> {
	builders::build_commit_bundle_from_sources(repo, commit, default_branch, notes)
}
