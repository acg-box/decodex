use std::path::Path;

use crate::{
	recovery::{
		AdoptValidation, REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_REBIND_EVENT, RebindMode,
		RebindValidation,
		tests::{self},
	},
	state::StateStore,
};

#[test]
fn adopt_private_event_records_manual_takeover_lifecycle_evidence() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let validation = AdoptValidation {
		issue: tests::sample_issue("In Review"),
		branch_name: branch_name.to_owned(),
		worktree_path: Path::new("/tmp/PUB-718").to_path_buf(),
		run_id: String::from("pub-718-manual-adopt-2-1123456789ab"),
		attempt_number: 2,
		landing_state: tests::sample_landing_state(pr_url, branch_name, head_oid),
		local_head_oid: head_oid.to_owned(),
		worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
		active_label_present: false,
		success_state_transition: None,
		previous_worktree_mapping: None,
	};

	super::append_review_handoff_adopt_private_event(
		&state_store,
		"pubfi",
		&validation,
		"local_markers_written",
		false,
	)
	.expect("adopt private event should append");
	super::append_review_handoff_adopt_private_event(
		&state_store,
		"pubfi",
		&validation,
		"active_label_checked",
		true,
	)
	.expect("adopt active-label private event should append");

	let events = state_store
		.list_private_execution_events(
			"pubfi",
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
		)
		.expect("private events should read");
	let event = events.first().expect("adopt event should exist");
	let payload = event.payload();
	let second_event = events.get(1).expect("active-label adopt event should exist");
	let second_payload = second_event.payload();

	assert_eq!(events.len(), 2);
	assert_eq!(event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
	assert_eq!(payload["event"], REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(payload["writeback_stage"], "local_markers_written");
	assert_eq!(payload["manual_takeover_adopt"], true);
	assert_eq!(payload["active_label_restored"], false);
	assert_eq!(payload["pr_url"], pr_url);
	assert_eq!(payload["pr_head_sha"], head_oid);
	assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
	assert_eq!(second_event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(second_payload["writeback_stage"], "active_label_checked");
	assert_eq!(second_payload["active_label_restored"], true);
}

#[test]
fn rebind_private_event_records_retained_lifecycle_evidence() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let validation = RebindValidation {
		issue: tests::sample_issue("In Review"),
		worktree: tests::sample_worktree(branch_name),
		run_id: String::from("pub-718-attempt-2-1123456789ab"),
		attempt_number: 2,
		landing_state: tests::sample_landing_state(pr_url, branch_name, head_oid),
		local_head_oid: head_oid.to_owned(),
		worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
		active_label_present: true,
		restore_active_label: false,
		mode: RebindMode::RefreshExistingHandoff,
		success_state_transition: None,
		clear_needs_attention_label: false,
	};

	super::append_review_handoff_rebind_private_event(
		&state_store,
		"pubfi",
		&validation,
		"local_markers_written",
		false,
	)
	.expect("rebind private event should append");

	let events = state_store
		.list_private_execution_events(
			"pubfi",
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
		)
		.expect("private events should read");
	let event = events.first().expect("rebind event should exist");
	let payload = event.payload();

	assert_eq!(events.len(), 1);
	assert_eq!(event.event_type(), REVIEW_HANDOFF_REBIND_EVENT);
	assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
	assert_eq!(payload["event"], REVIEW_HANDOFF_REBIND_EVENT);
	assert_eq!(payload["writeback_stage"], "local_markers_written");
	assert_eq!(payload["mode"], "refresh_existing_handoff");
	assert_eq!(payload["active_label_present"], true);
	assert_eq!(payload["active_label_restored"], false);
	assert_eq!(payload["pr_url"], pr_url);
	assert_eq!(payload["pr_head_sha"], head_oid);
	assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
}
