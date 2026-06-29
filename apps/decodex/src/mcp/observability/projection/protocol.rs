use serde_json::Value;

use crate::mcp::observability::sanitizer;

pub(super) fn mcp_public_protocol_activity(run: &Value) -> Value {
	let mut activity = run.get("protocol_activity").cloned().unwrap_or(Value::Null);

	sanitizer::redact_reasoning_protocol_activity(&mut activity);

	activity
}
