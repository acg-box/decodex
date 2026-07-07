use serde_json::Value;

use crate::state::PrivateExecutionEvent;

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_failed_control_attempt(
	event: &PrivateExecutionEvent,
) -> bool {
	if event.event_type() == "lane_control/interrupt" {
		return event.payload().get("processAliveAfter").and_then(Value::as_bool) == Some(false)
			&& event.payload().get("status").and_then(Value::as_str) == Some("sent");
	}

	event.event_type() == "control_action"
		&& matches!(
			event.payload().get("action").and_then(Value::as_str),
			Some("interrupt" | "steer")
		) && matches!(
		event.payload().get("reason").and_then(Value::as_str),
		Some(
			"run_lease_missing"
				| "hard_fallback_unavailable"
				| "hard_interrupt_fallback"
				| "process_not_signalable"
		)
	)
}

pub(in crate::recovery::evidence::stale_active) fn stale_active_private_event_is_dead_process_control_telemetry(
	event: &PrivateExecutionEvent,
) -> bool {
	match event.event_type() {
		"lane_control/interrupt/requested" => {
			event.payload().get("method").and_then(Value::as_str) == Some("turn/interrupt")
		},
		"control_action" => {
			let payload = event.payload();

			payload.get("schema").and_then(Value::as_str) == Some("decodex.run_control_action/v1")
				&& payload.get("action").and_then(Value::as_str) == Some("interrupt")
				&& matches!(
					payload.get("reason").and_then(Value::as_str),
					Some(
						"run_lease_control_channel_resolved"
							| "soft_interrupt_response_pending"
							| "hard_interrupt_fallback"
					)
				) && matches!(
				payload.get("outcome").and_then(Value::as_str),
				Some("accepted" | "timed_out" | "fallback")
			) && payload.pointer("/context/process_alive").and_then(Value::as_bool) == Some(false)
		},
		_ => false,
	}
}
