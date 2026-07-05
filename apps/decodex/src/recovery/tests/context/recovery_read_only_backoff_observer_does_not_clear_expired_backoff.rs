use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	recovery::{
		self, LINEAR_RATE_LIMIT_BACKOFF_WARNING, RecoveryRuntimeMutationPolicy,
		tests::{self},
	},
	state::ConnectorBackoffInput,
};

#[test]
fn recovery_read_only_backoff_observer_does_not_clear_expired_backoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let expired_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() - 1;

	context
		.state_store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: context.config.service_id(),
			connector: "linear",
			sync_phase: "ghost_lane_recovery",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch: expired_unix_epoch,
			reset_source: "test",
			warning: LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		})
		.expect("backoff should persist");

	let message = recovery::active_recovery_tracker_backoff_message(&context)
		.expect("backoff observer should run");

	assert_eq!(message, None);
	assert!(
		context
			.state_store
			.connector_backoff(context.config.service_id(), "linear")
			.expect("backoff should read")
			.is_some(),
		"read-only recovery diagnostics must not clear stored connector backoff"
	);
}
