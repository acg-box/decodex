use crate::{
	recovery::{
		self, RebindMode,
		tests::{self, GhostLaneTestTracker},
	},
	tracker::TrackerLabel,
};

#[test]
fn rebind_label_validation_restores_active_and_clears_attention_for_missing_writeback_failure() {
	let workflow = tests::sample_workflow();
	let mut issue =
		tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);

	issue.team.labels.push(TrackerLabel {
		id: String::from("label-decodex-active-pubfi"),
		name: String::from("decodex:active:pubfi"),
	});

	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let labels = recovery::validate_rebind_tracker_labels_with_tracker(
		&tracker,
		"pubfi",
		workflow.frontmatter().tracker(),
		&issue,
		RebindMode::RestoreMissingHandoffAfterWritebackFailure,
	)
	.expect("writeback-failure missing handoff should restore ownership and clear attention");

	assert!(!labels.active_label_present);
	assert!(labels.restore_active_label);
	assert!(labels.clear_needs_attention_label);
}
