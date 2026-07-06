//! Reentry predicates for stale-active release recovery.

mod apply;
mod control;
mod local_cleanup;
mod startable_restore;
mod types;

pub(super) use self::{
	apply::apply_stale_active_release_reentries, types::StaleActiveReleaseReentryInput,
};

pub(in crate::recovery) fn evidence_contains(evidence: &[String], expected: &str) -> bool {
	evidence.iter().any(|entry| entry == expected)
}
