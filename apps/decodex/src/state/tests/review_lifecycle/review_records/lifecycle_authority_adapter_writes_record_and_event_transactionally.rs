use std::path::Path;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		PostReviewLifecycleFactsInput, PullRequestReviewState, build_post_review_lifecycle_facts,
		kernel::lifecycle::{
			LIFECYCLE_EVENT_SCHEMA_VERSION, LIFECYCLE_EVENT_TYPE, LifecycleDecisionInput,
			LifecycleEvidenceKind, LifecycleOutcome, decide_lifecycle_transition,
		},
	},
	state::{ReviewLifecycleHandoffFixture, ReviewLifecycleRecord, StateStore},
};

#[test]
fn lifecycle_authority_adapter_writes_record_and_event_transactionally() {
	let store = StateStore::open_in_memory().expect("state store should open");
	let handoff = ReviewLifecycleHandoffFixture::new(
		"run-1",
		1,
		"x/pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/pub-101",
		"head-sha",
	);
	let lifecycle_record = ReviewLifecycleRecord::from_test_lifecycle_fixtures(&handoff, None);
	let review_state = PullRequestReviewState {
		url: String::from("https://github.com/hack-ink/decodex/pull/101"),
		state: String::from("OPEN"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		merge_commit_allowed: true,
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		head_ref_name: String::from("x/pub-101"),
		head_ref_oid: String::from("head-sha"),
		merge_commit_oid: None,
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: Some(String::from("SUCCESS")),
		unresolved_review_threads: 0,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	};
	let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: "pubfi",
		issue_id: "PUB-101",
		review_lifecycle: Some(&lifecycle_record),
		review_state: &review_state,
		worktree_path: Path::new("/tmp/pubfi"),
		review_level: ReviewLevel::Standard,
		phase: "landing",
		landing_state: None,
		closeout_state: None,
		validated_head_sha: Some("head-sha"),
		review_checkpoint_phase: Some("handoff"),
		review_checkpoint_status: Some("clean"),
	});
	let decision = decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous: None,
		evidence_kind: LifecycleEvidenceKind::LandingReadback,
		outcome: LifecycleOutcome::Succeeded,
		merge_commit: Some("merge-sha"),
		cleanup_state: Some("pending"),
		authority: "issue_authority",
		actor: "runtime",
		idempotency_key: "PUB-101:landed:merge-sha",
		correlation_id: "corr-1",
		causation_id: Some("landing-intent-1"),
		decided_at: "2026-07-07T00:00:00Z",
	});

	let event = store
		.record_lifecycle_decision("run-1", 1, &decision)
		.expect("lifecycle decision should persist");

	assert_eq!(event.event_type(), LIFECYCLE_EVENT_TYPE);
	assert_eq!(event.payload()["schema_version"], LIFECYCLE_EVENT_SCHEMA_VERSION);
	assert_eq!(event.payload()["authority_record"]["transition"], "landed");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read")
		.expect("authority record should exist");

	assert_eq!(lifecycle.sequence(), 1);
	assert_eq!(lifecycle.transition(), "landed");
	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.landing_state(), "landed");
	assert_eq!(lifecycle.merge_commit(), Some("merge-sha"));
	assert_eq!(lifecycle.authority(), "issue_authority");
	assert_eq!(lifecycle.actor(), "runtime");
	assert_eq!(lifecycle.idempotency_key(), "PUB-101:landed:merge-sha");
	assert!(lifecycle.evidence_json().contains("lifecycle_authority_recorded"));

	let duplicate = store
		.record_lifecycle_decision("run-1", 1, &decision)
		.expect("duplicate lifecycle decision should be idempotent");
	let events = store
		.list_private_execution_events_for_issue("pubfi", "PUB-101")
		.expect("events should list");

	assert_eq!(duplicate.record_id(), event.record_id());
	assert_eq!(events.len(), 1);

	store
		.upsert_worktree("pubfi", "PUB-101", "x/pub-101", "/tmp/pubfi")
		.expect("worktree mapping should persist");
	store.clear_worktree("PUB-101").expect("worktree clear should preserve authority");

	let preserved_lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read after cleanup")
		.expect("authority record should survive worktree cleanup");

	assert_eq!(preserved_lifecycle.sequence(), 1);
	assert_eq!(preserved_lifecycle.next_state(), "landed");
}
