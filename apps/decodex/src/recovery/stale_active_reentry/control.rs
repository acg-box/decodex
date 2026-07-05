use crate::recovery::stale_active_reentry::evidence;

pub(in crate::recovery::stale_active_reentry) fn reentry_control_channel_inactive_or_absent(
	control_channel: &str,
	audit_evidence: &[String],
) -> bool {
	if control_channel == "missing" {
		return evidence::evidence_contains(audit_evidence, "control_channel_missing");
	}

	!control_channel.ends_with(":active")
		&& evidence::evidence_contains(audit_evidence, "control_channel_inactive_or_file_missing")
}
