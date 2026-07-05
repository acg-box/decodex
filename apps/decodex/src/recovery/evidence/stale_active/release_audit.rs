use serde_json::Value;

use crate::{
	recovery::{STALE_ACTIVE_RECOVERY_SCHEMA, STALE_ACTIVE_RELEASE_EVENT},
	state::PrivateExecutionEvent,
};

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_release_audit(
	event: &PrivateExecutionEvent,
) -> bool {
	event.event_type() == STALE_ACTIVE_RELEASE_EVENT
		&& event.payload().get("schema").and_then(Value::as_str)
			== Some(STALE_ACTIVE_RECOVERY_SCHEMA)
		&& event.payload().get("event").and_then(Value::as_str) == Some(STALE_ACTIVE_RELEASE_EVENT)
}
