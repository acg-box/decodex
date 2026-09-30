//! GitHub source payload normalization for Radar bundle artifacts.

mod builders;
mod evidence;
mod extraction;
mod fields;
mod items;
mod refs;
mod validation;

pub(crate) use self::{
	builders::{build_commit_bundle_from_sources, build_pr_bundle_from_sources},
	evidence::install_bundle,
};
#[cfg(test)] pub(crate) use evidence::install_bundle_after_write;
