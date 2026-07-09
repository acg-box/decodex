use std::path::Path;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		PostReviewLifecycleFactsInput, PullRequestReviewState, build_post_review_lifecycle_facts,
		kernel::lifecycle::{
			LifecycleDecisionInput, LifecycleEvidenceKind, LifecycleOutcome,
			PreviousLifecycleAuthority, decide_lifecycle_transition,
		},
	},
	recovery::tests::review_handoff,
	state::{
		ReviewLifecycleHandoffFixture, ReviewLifecycleHandoffInput, ReviewLifecycleTransitionInput,
		StateStore,
	},
};

fn merged_review_state(pr_url: &str, branch_name: &str, head_oid: &str) -> PullRequestReviewState {
	PullRequestReviewState {
		url: pr_url.to_owned(),
		state: String::from("MERGED"),
		is_draft: false,
		review_decision: Some(String::from("APPROVED")),
		merge_commit_allowed: true,
		pending_review_requests: 0,
		mergeable: String::from("MERGEABLE"),
		merge_state_status: String::from("CLEAN"),
		base_ref_oid: Some(String::from("base-sha")),
		head_ref_name: branch_name.to_owned(),
		head_ref_oid: head_oid.to_owned(),
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
fn rebind_validation_preserves_terminal_landed_lifecycle_authority() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let handoff = ReviewLifecycleHandoffFixture::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	state_store
		.upsert_review_lifecycle_handoff_fixture("pubfi", "issue-id", &handoff)
		.expect("handoff authority should persist");
	let lifecycle_record = state_store
		.review_lifecycle_record("pubfi", "issue-id", branch_name)
		.expect("authority record should read")
		.expect("authority record should exist");
	let review_state = merged_review_state(pr_url, branch_name, head_oid);
	let facts = build_post_review_lifecycle_facts(PostReviewLifecycleFactsInput {
		project_id: "pubfi",
		issue_id: "issue-id",
		review_lifecycle: Some(&lifecycle_record),
		review_state: &review_state,
		worktree_path: Path::new("/tmp/pubfi"),
		review_level: ReviewLevel::Standard,
		phase: "landing",
		landing_state: Some("landed"),
		closeout_state: None,
		validated_head_sha: Some(head_oid),
		review_checkpoint_phase: Some("handoff"),
		review_checkpoint_status: Some("clean"),
	});
	let landed_decision = decide_lifecycle_transition(LifecycleDecisionInput {
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
		idempotency_key: "PUB-718:landed:merge-sha",
		correlation_id: "pub-718-attempt-1",
		causation_id: Some("landing_complete"),
		decided_at: "2026-07-07T00:00:00Z",
	});
	state_store
		.record_lifecycle_decision("pub-718-attempt-1", 1, &landed_decision)
		.expect("landed authority should persist");
	let event_count_before_rebind = state_store
		.list_private_execution_events_for_issue("pubfi", "issue-id")
		.expect("events should list")
		.len();

	review_handoff::write_review_lifecycle_with_rollback(
		&state_store,
		"pubfi",
		"issue-id",
		ReviewLifecycleHandoffInput {
			run_id: "pub-718-attempt-1",
			attempt_number: 1,
			branch_name,
			pr_url,
			base_ref_name: "main",
			head_ref_name: branch_name,
			head_sha: head_oid,
		},
		ReviewLifecycleTransitionInput {
			run_id: "pub-718-attempt-1",
			attempt_number: 1,
			branch_name,
			pr_url,
			head_sha: head_oid,
			phase: "request_pending",
			request_comment_database_id: None,
			request_created_at_unix_epoch: None,
			request_description_thumbs_up_count: None,
			request_retry_count: 0,
			external_round_count: 0,
			auto_merge_enabled_at_unix_epoch: None,
		},
	)
	.expect("stale rebind lifecycle write should be ignored");

	let lifecycle = state_store
		.review_lifecycle_record("pubfi", "issue-id", branch_name)
		.expect("authority record should read")
		.expect("authority record should remain");
	let event_count_after_rebind = state_store
		.list_private_execution_events_for_issue("pubfi", "issue-id")
		.expect("events should list")
		.len();

	assert_eq!(lifecycle.next_state(), "landed");
	assert_eq!(lifecycle.transition(), "landed");
	assert_eq!(lifecycle.phase(), "landed");
	assert_eq!(lifecycle.next_action(), "run_retained_closeout_adapter");
	assert_eq!(event_count_after_rebind, event_count_before_rebind);
}
