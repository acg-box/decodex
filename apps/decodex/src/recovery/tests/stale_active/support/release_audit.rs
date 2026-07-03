use crate::{
	recovery::{STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT},
	state::StateStore,
};

pub(in crate::recovery::tests::stale_active) fn append_stale_active_release_audit(
	store: &StateStore,
	issue_id: &str,
) {
	append_stale_active_release_audit_for_run(store, issue_id, "run-1626", 1);
}

pub(in crate::recovery::tests::stale_active) fn append_stale_active_release_audit_for_run(
	store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) {
	store
		.append_private_execution_event(
			"pubfi",
			issue_id,
			run_id,
			attempt_number,
			STALE_ACTIVE_RELEASE_EVENT,
			serde_json::json!({
				"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
				"event": STALE_ACTIVE_RELEASE_EVENT,
				"phase": "local_cleanup_complete_before_active_label_release",
			}),
		)
		.expect("stale active release audit should record");
}
