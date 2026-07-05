//! Reentry predicates for stale-active release recovery.

mod apply;
mod control;
mod evidence;
mod local_cleanup;
mod startable_restore;
mod types;

pub(super) use self::{
	apply::apply_stale_active_release_reentries, evidence::evidence_contains,
	types::StaleActiveReleaseReentryInput,
};
