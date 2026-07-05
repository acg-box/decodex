use tempfile::TempDir;

use crate::{
	prelude::eyre,
	recovery::{
		self, RecoveryRuntimeMutationPolicy,
		tests::{self},
	},
};

#[test]
fn recovery_read_only_backoff_recorder_does_not_persist_new_backoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let error = eyre::eyre!("Linear connector timed out while testing");
	let message = recovery::remember_recovery_tracker_backoff_message(
		&context,
		&error,
		"ghost_lane_recovery",
	)
	.expect("timeout should produce backoff message");

	assert!(message.contains("Linear connector is in backoff"));
	assert!(
		context
			.state_store
			.connector_backoff(context.config.service_id(), "linear")
			.expect("backoff should read")
			.is_none(),
		"read-only recovery diagnostics must not persist new connector backoff"
	);
}
