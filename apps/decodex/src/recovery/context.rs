//! Recovery runtime context loading and tracker-backoff helpers.

mod backoff;
mod loader;
mod paths;
mod policy;

pub(in crate::recovery) use self::{
	backoff::{active_recovery_tracker_backoff_message, remember_recovery_tracker_backoff_message},
	loader::{load_recovery_context_for_dry_run, load_recovery_context_read_only},
	policy::RecoveryRuntimeMutationPolicy,
};

use crate::{
	config::ServiceConfig, state::StateStore, tracker::linear::LinearClient,
	workflow::WorkflowDocument,
};

pub(super) const LINEAR_RATE_LIMIT_BACKOFF_WARNING: &str = "tracker_rate_limited";

pub(super) struct RecoveryContext {
	pub(super) config: ServiceConfig,
	pub(super) workflow: WorkflowDocument,
	pub(super) state_store: StateStore,
	pub(super) tracker: LinearClient,
	pub(super) runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
}
