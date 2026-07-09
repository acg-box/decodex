use std::path::Path;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, PostReviewLifecycleFactsInput, PullRequestReviewState,
		kernel::{
			lifecycle,
			lifecycle::{
				LIFECYCLE_EVENT_SCHEMA_VERSION, LIFECYCLE_EVENT_TYPE, LifecycleDecisionInput,
				LifecycleEvidenceKind, LifecycleOutcome, PreviousLifecycleAuthority,
			},
		},
	},
	state::{
		ReviewLifecycleHandoffFixture, ReviewLifecycleHandoffInput, ReviewLifecycleRecord,
		ReviewLifecycleTransitionInput, StateStore,
	},
};

fn pub_101_merged_review_state() -> PullRequestReviewState {
	PullRequestReviewState {
		url: String::from("https://github.com/hack-ink/decodex/pull/101"),
		state: String::from("MERGED"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		merge_commit_allowed: true,
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		base_ref_oid: Some(String::from("base-sha")),
		head_ref_name: String::from("x/pub-101"),
		head_ref_oid: String::from("head-sha"),
		merge_commit_oid: Some(String::from("merge-sha")),
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: Some(String::from("SUCCESS")),
		required_status_contexts: Vec::new(),
		unresolved_review_threads: 0,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	}
}

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
		base_ref_oid: Some(String::from("base-sha")),
		head_ref_name: String::from("x/pub-101"),
		head_ref_oid: String::from("head-sha"),
		merge_commit_oid: None,
		head_repository_name: None,
		head_repository_owner: None,
		status_check_rollup_state: Some(String::from("SUCCESS")),
		required_status_contexts: Vec::new(),
		unresolved_review_threads: 0,
		issue_description_external_review_thumbs_up_count: 0,
		issue_comments: Vec::new(),
		reviews: Vec::new(),
	};
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
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
	let decision = lifecycle::decide_lifecycle_transition(LifecycleDecisionInput {
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

#[test]
fn review_wait_sync_does_not_regress_terminal_lifecycle_authority() {
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

	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &handoff)
		.expect("handoff authority should persist");

	let lifecycle_record = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read")
		.expect("authority record should exist");
	let review_state = pub_101_merged_review_state();
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: "pubfi",
		issue_id: "PUB-101",
		review_lifecycle: Some(&lifecycle_record),
		review_state: &review_state,
		worktree_path: Path::new("/tmp/pubfi"),
		review_level: ReviewLevel::Standard,
		phase: "closeout",
		landing_state: Some("landed"),
		closeout_state: Some("completed"),
		validated_head_sha: Some("head-sha"),
		review_checkpoint_phase: Some("handoff"),
		review_checkpoint_status: Some("clean"),
	});
	let closeout_decision = lifecycle::decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous: Some(PreviousLifecycleAuthority {
			sequence: lifecycle_record.sequence(),
			next_state: lifecycle_record.next_state(),
		}),
		evidence_kind: LifecycleEvidenceKind::CloseoutCompletion,
		outcome: LifecycleOutcome::Succeeded,
		merge_commit: Some("merge-sha"),
		cleanup_state: Some("completed"),
		authority: "issue_authority",
		actor: "manual_land",
		idempotency_key: "PUB-101:closeout:merge-sha",
		correlation_id: "run-1",
		causation_id: Some("manual_land_closeout_complete"),
		decided_at: "2026-07-07T00:00:00Z",
	});

	store
		.record_lifecycle_decision("run-1", 1, &closeout_decision)
		.expect("closeout authority should persist");

	let event_count_before_stale_sync = store
		.list_private_execution_events_for_issue("pubfi", "PUB-101")
		.expect("events should list")
		.len();

	store
		.record_review_lifecycle_transition(
			"pubfi",
			"PUB-101",
			ReviewLifecycleTransitionInput {
				run_id: "run-1",
				attempt_number: 1,
				branch_name: "x/pub-101",
				pr_url: "https://github.com/hack-ink/decodex/pull/101",
				head_sha: "head-sha",
				phase: "request_pending",
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 1,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
			},
		)
		.expect("stale review-wait sync should be ignored");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read")
		.expect("authority record should remain");
	let event_count_after_stale_sync = store
		.list_private_execution_events_for_issue("pubfi", "PUB-101")
		.expect("events should list")
		.len();

	assert_eq!(lifecycle.next_state(), "closed");
	assert_eq!(lifecycle.transition(), "closeout_completed");
	assert_eq!(lifecycle.next_action(), "no_action");
	assert_eq!(lifecycle.phase(), "closed");
	assert_eq!(event_count_after_stale_sync, event_count_before_stale_sync);
}

#[test]
fn review_handoff_sync_does_not_regress_landed_lifecycle_authority() {
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

	store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "PUB-101", &handoff)
		.expect("handoff authority should persist");

	let lifecycle_record = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read")
		.expect("authority record should exist");
	let review_state = pub_101_merged_review_state();
	let facts = orchestrator::build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: "pubfi",
		issue_id: "PUB-101",
		review_lifecycle: Some(&lifecycle_record),
		review_state: &review_state,
		worktree_path: Path::new("/tmp/pubfi"),
		review_level: ReviewLevel::Standard,
		phase: "landing",
		landing_state: Some("landed"),
		closeout_state: None,
		validated_head_sha: Some("head-sha"),
		review_checkpoint_phase: Some("handoff"),
		review_checkpoint_status: Some("clean"),
	});
	let landed_decision = lifecycle::decide_lifecycle_transition(LifecycleDecisionInput {
		facts: &facts,
		previous: Some(PreviousLifecycleAuthority {
			sequence: lifecycle_record.sequence(),
			next_state: lifecycle_record.next_state(),
		}),
		evidence_kind: LifecycleEvidenceKind::LandingReadback,
		outcome: LifecycleOutcome::Succeeded,
		merge_commit: Some("merge-sha"),
		cleanup_state: Some("pending"),
		authority: "issue_authority",
		actor: "runtime",
		idempotency_key: "PUB-101:landed:merge-sha",
		correlation_id: "run-1",
		causation_id: Some("landing_complete"),
		decided_at: "2026-07-07T00:00:00Z",
	});

	store
		.record_lifecycle_decision("run-1", 1, &landed_decision)
		.expect("landed authority should persist");

	let event_count_before_stale_sync = store
		.list_private_execution_events_for_issue("pubfi", "PUB-101")
		.expect("events should list")
		.len();

	store
		.record_review_lifecycle_handoff(
			"pubfi",
			"PUB-101",
			ReviewLifecycleHandoffInput {
				run_id: "run-1",
				attempt_number: 1,
				branch_name: "x/pub-101",
				pr_url: "https://github.com/hack-ink/decodex/pull/101",
				base_ref_name: "main",
				head_ref_name: "x/pub-101",
				head_sha: "head-sha",
			},
		)
		.expect("stale handoff sync should be ignored");
	store
		.record_review_lifecycle_transition(
			"pubfi",
			"PUB-101",
			ReviewLifecycleTransitionInput {
				run_id: "run-1",
				attempt_number: 1,
				branch_name: "x/pub-101",
				pr_url: "https://github.com/hack-ink/decodex/pull/101",
				head_sha: "head-sha",
				phase: "request_pending",
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 1,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
			},
		)
		.expect("stale review-wait sync should be ignored");

	let lifecycle = store
		.review_lifecycle_record("pubfi", "PUB-101", "x/pub-101")
		.expect("authority record should read")
		.expect("authority record should remain");
	let event_count_after_stale_sync = store
		.list_private_execution_events_for_issue("pubfi", "PUB-101")
		.expect("events should list")
		.len();

	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.transition(), "landed");
	assert_eq!(lifecycle.next_action(), "run_retained_closeout_adapter");
	assert_eq!(lifecycle.phase(), "landed");
	assert_eq!(event_count_after_stale_sync, event_count_before_stale_sync);
}
