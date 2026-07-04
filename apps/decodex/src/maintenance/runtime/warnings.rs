use color_eyre::Report;

use crate::maintenance::reports::RuntimeMaintenanceWarning;

pub(in crate::maintenance::runtime) fn runtime_maintenance_warning_for_error(
	error: &Report,
) -> RuntimeMaintenanceWarning {
	let message = error.to_string().to_ascii_lowercase();
	let reason =
		if message.contains("busy") || message.contains("locked") || message.contains("sqlite") {
			"sqlite_unavailable"
		} else {
			"candidate_detection_failed"
		};

	RuntimeMaintenanceWarning { warning: "auto_protocol_event_compaction_skipped", reason }
}
