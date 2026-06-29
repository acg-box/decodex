mod reasoning;
mod sensitive_text;
mod structured;

use serde_json::Value;

pub(super) fn redact_reasoning_protocol_activity(value: &mut Value) {
	reasoning::redact_reasoning_protocol_activity(value);
}

pub(in crate::mcp) fn sanitize_mcp_observability_value(value: &mut Value) {
	structured::sanitize_mcp_observability_value(value);
}

pub(in crate::mcp) fn mcp_sanitized_value(value: Value) -> Value {
	structured::mcp_sanitized_value(value)
}
