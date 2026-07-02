use color_eyre::eyre::Report;

use crate::agent::app_server;

pub(in crate::agent::app_server::lane_control) fn soft_interrupt_error_class(
	error: &Report,
) -> &'static str {
	if app_server::is_app_server_output_timeout(error) {
		return "soft_interrupt_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("-32601") || error_text.contains("method not found") {
		"soft_interrupt_unsupported"
	} else {
		"soft_interrupt_failed"
	}
}

pub(in crate::agent::app_server) fn steer_error_class(error: &Report) -> &'static str {
	if app_server::is_app_server_output_timeout(error) {
		return "app_server_turn_steer_timed_out";
	}

	let error_text = error.to_string().to_ascii_lowercase();

	if error_text.contains("activeturnnotsteerable")
		|| error_text.contains("active turn not steerable")
	{
		return "active_turn_not_steerable";
	}
	if error_text.contains("-32601") || error_text.contains("method not found") {
		return "app_server_turn_steer_unsupported";
	}

	"app_server_turn_steer_failed"
}
