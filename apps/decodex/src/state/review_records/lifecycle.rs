mod authority;
mod cleanup;
mod handoff;
mod orchestration;
mod queries;

fn terminal_lifecycle_authority_must_not_reenter_review(
	record: &crate::state::ReviewLifecycleRecord,
) -> bool {
	matches!(record.next_state(), "landed" | "closed")
}
