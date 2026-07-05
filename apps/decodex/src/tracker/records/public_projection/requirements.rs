pub(in crate::tracker::records::public_projection) fn event_requires_summary(
	event_type: &str,
) -> bool {
	matches!(
		event_type,
		"run_started"
			| "intake"
			| "progress_checkpoint"
			| "review_handoff"
			| "repair_handoff"
			| "review_handoff_rebind"
			| "review_handoff_adopt"
			| "landed"
			| "closeout"
			| "cleanup_complete"
	)
}

pub(in crate::tracker::records::public_projection) fn event_requires_next_action(
	event_type: &str,
) -> bool {
	matches!(event_type, "needs_attention" | "terminal_failure")
}

pub(in crate::tracker::records::public_projection) fn event_requires_items(
	event_type: &str,
	field_name: &str,
) -> bool {
	matches!(
		(event_type, field_name),
		(
			"needs_attention"
				| "terminal_failure"
				| "review_handoff_rebind"
				| "review_handoff_adopt",
			"evidence",
		) | ("needs_attention" | "terminal_failure", "blockers")
	)
}
