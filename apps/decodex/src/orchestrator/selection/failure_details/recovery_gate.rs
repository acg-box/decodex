pub(crate) fn terminal_failure_recovery_gate(
	needs_attention_label: &str,
	needs_attention_label_available: bool,
	guarded_by_nonstartable_state: bool,
	nonstartable_guard_state: &str,
) -> String {
	if needs_attention_label_available {
		return format!(
			"clear label `{needs_attention_label}`, then move the issue back to a startable state if another automated run is desired"
		);
	}
	if guarded_by_nonstartable_state {
		return format!(
			"`{needs_attention_label}` could not be applied because it does not exist on the team; the issue remains in `{nonstartable_guard_state}` to block automatic retries, so move it back to a startable state manually if another automated run is desired"
		);
	}

	format!(
		"`{needs_attention_label}` could not be applied because it does not exist on the team; move the issue back to a startable state manually if another automated run is desired"
	)
}
