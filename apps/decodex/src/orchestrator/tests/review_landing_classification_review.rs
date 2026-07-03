use std::fs;

use tempfile::TempDir;

use crate::{
	orchestrator::{
		self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
		AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
		AuthorityDecisionRequestInput, PostReviewLaneClassification, PostReviewLaneDecision,
		PostReviewLaneSnapshot, ReviewOrchestrationMarker, StateStore, tests,
		tests::{
			FakePullRequestReviewStateInspector, TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID,
			TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT, TEST_SERVICE_ID,
		},
	},
	tracker::TrackerIssue,
	worktree::WorktreeManager,
};

pub(super) fn record_block_landing_authority_boundary(
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	record_policy_authority_boundary(
		state_store,
		issue,
		AuthorityBoundarySurface::ReviewPolicy,
		AuthorityBoundaryPolicyDecision::BlockLanding,
		"review_churn",
		"Review policy changed during recovery.",
		"Review policy evidence must be restored before landing.",
	);
}

pub(super) fn record_requires_human_authority_boundary(
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	record_policy_authority_boundary(
		state_store,
		issue,
		AuthorityBoundarySurface::AuthorityEvidence,
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
		"authority_gap",
		"Operator authority evidence is required before landing.",
		"Operator acceptance must be recorded before landing.",
	);
}

pub(super) fn record_authority_decision_request(state_store: &StateStore, issue: &TrackerIssue) {
	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "run-boundary",
			attempt_number: 1,
			decision_contract_ids: Vec::new(),
			attempted_recovery_reason: "authority_gap",
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface: AuthorityBoundarySurface::AuthorityEvidence,
				change_summary: "Operator authority evidence is required before landing.",
				policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition: AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Operator acceptance must be recorded before landing.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("authority boundary check should persist");

	orchestrator::record_authority_decision_request_private_event(
		state_store,
		AuthorityDecisionRequestInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "run-boundary",
			attempt_number: 1,
			boundary_check_record_id: boundary_event.record_id(),
			decision_request_id: "dr-pub-101-1",
			reason_code: "authority_evidence_required",
			boundary_type: "operator_acceptance",
			proposed_change: "Land only after operator acceptance.",
			why_exceeds_authority: "The current lane requires an explicit operator decision.",
			options: vec![orchestrator::AuthorityDecisionOption {
				label: "accept",
				description: "Record operator acceptance before resuming.",
			}],
			recommendation: "Record operator acceptance before resuming automation.",
			resume_condition: "Resume only after the issue, Decision Contract, or policy records the operator decision.",
			retained_worktree_evidence: vec!["retained worktree has a PR-ready head"],
			retained_diff_evidence: vec!["diff evidence retained privately"],
			recovery_attempt_context: vec!["landing stopped at the authority boundary"],
		},
	)
	.expect("authority decision request should persist");
}

#[test]
fn classify_post_review_lane_requires_review_repair_for_unresolved_threads() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				2,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "unresolved_review_threads");
}

#[test]
fn classify_post_review_lane_ignores_stale_review_orchestration_record_from_prior_handoff() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");

	state_store
		.upsert_review_orchestration_marker(
			"pubfi",
			&issue.id,
			&ReviewOrchestrationMarker::new(
				"run-0",
				7,
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/99",
				"deadbeef",
				"waiting_for_result",
				Some(TEST_EXTERNAL_REVIEW_REQUEST_COMMENT_ID),
				Some(TEST_EXTERNAL_REVIEW_REQUEST_CREATED_AT),
				Some(1),
				2,
				3,
				None,
			),
		)
		.expect("stale review orchestration marker should persist");

	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_request_pending");
}

#[test]
fn classify_post_review_lane_request_pending_waits_for_green_checks_before_external_review_request()
{
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("PENDING"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_request_waiting_for_green_checks",);
}

#[test]
fn classify_post_review_lane_request_pending_routes_fixable_ci_red_to_repair() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"BLOCKED",
				Some("FAILURE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "required_checks_failed");
}

#[test]
fn classify_post_review_lane_request_pending_repairs_unhandled_ci_red() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"UNSTABLE",
				Some("FAILURE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "external_review_request_ci_red_repair_required",);
}

#[test]
fn classify_post_review_lane_request_pending_waits_for_unknown_check_state() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&ReviewOrchestrationMarker::new(
			"run-1",
			1,
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		),
	);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"UNSTABLE",
				Some("UNKNOWN_NEW_STATE"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
	assert_eq!(classification.reason, "external_review_request_waiting_for_green_checks",);
}

#[test]
fn classify_post_review_lane_requires_review_repair_for_non_thread_review_summary_findings() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		Some("COMMENTED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_findings(&mut review_state, "Please cover the failing edge case.");

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "external_review_feedback_pending_repair");
}

#[test]
fn classify_post_review_lane_ready_to_land_allows_zero_required_review_repos() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::ReadyToLand);
	assert_eq!(classification.reason, "external_review_passed_strict");
}

#[test]
fn classify_post_review_lane_blocks_landing_for_unresolved_authority_boundary() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees(TEST_SERVICE_ID)
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	record_block_landing_authority_boundary(&state_store, &snapshot.issue);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone())]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "authority_boundary_blocks_landing");

	record_clean_review_checkpoint_for_head(&state_store, &snapshot.issue.id, &head_oid);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::ReadyToLand);
	assert_eq!(classification.reason, "external_review_passed_strict");
}

#[test]
fn classify_post_review_lane_requires_enhanced_evidence_for_authority_boundary() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees(TEST_SERVICE_ID)
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	record_requires_enhanced_evidence_authority_boundary(&state_store, &snapshot.issue);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state.clone())]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::NeedsReviewRepair);
	assert_eq!(classification.reason, "authority_boundary_requires_enhanced_evidence");

	record_clean_review_checkpoint_for_head(&state_store, &snapshot.issue.id, &head_oid);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::ReadyToLand);
	assert_eq!(classification.reason, "external_review_passed_strict");
}

#[test]
fn classify_post_review_lane_blocks_for_human_decision_authority_boundary() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			TEST_SERVICE_ID,
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees(TEST_SERVICE_ID)
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};

	tests::seed_review_orchestration_marker(
		&state_store,
		TEST_SERVICE_ID,
		&snapshot.issue.id,
		&tests::sample_review_orchestration_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
			"waiting_for_result",
			1,
		),
	);

	record_requires_human_authority_boundary(&state_store, &snapshot.issue);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "authority_boundary_requires_human_decision");

	record_clean_review_checkpoint_for_head(&state_store, &snapshot.issue.id, &head_oid);

	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		None,
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	tests::add_external_review_ack(&mut review_state);
	tests::add_external_review_pass(&mut review_state);

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "authority_boundary_requires_human_decision");
}

fn record_requires_enhanced_evidence_authority_boundary(
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	record_policy_authority_boundary(
		state_store,
		issue,
		AuthorityBoundarySurface::PublicApi,
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence,
		"validation_repeat",
		"Public API changed during recovery.",
		"Public API changes require enhanced evidence before landing.",
	);
}

fn record_policy_authority_boundary(
	state_store: &StateStore,
	issue: &TrackerIssue,
	surface: AuthorityBoundarySurface,
	policy_decision: AuthorityBoundaryPolicyDecision,
	attempted_recovery_reason: &str,
	change_summary: &str,
	final_disposition_reason: &str,
) {
	orchestrator::record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: TEST_SERVICE_ID,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: "run-boundary",
			attempt_number: 1,
			decision_contract_ids: Vec::new(),
			attempted_recovery_reason,
			changed_surfaces: vec![AuthorityBoundaryChangedSurface {
				surface,
				change_summary,
				policy_decision,
				legacy_disposition: policy_decision.disposition(),
			}],
			policy_decision,
			disposition: policy_decision.disposition(),
			final_disposition_reason,
			improvement_signals: Vec::new(),
		},
	)
	.expect("authority boundary check should persist");
}

fn record_clean_review_checkpoint_for_head(
	state_store: &StateStore,
	issue_id: &str,
	head_oid: &str,
) {
	state_store
		.append_private_execution_event(
			TEST_SERVICE_ID,
			issue_id,
			"run-review",
			2,
			"review_checkpoint",
			serde_json::json!({
				"phase": "handoff",
				"status": "clean",
				"head_sha": head_oid,
				"nonclean_rounds": 0,
				"review": {
					"accepted_findings": [],
					"rejected_findings": [],
				}
			}),
		)
		.expect("clean review checkpoint should persist");
}

#[test]
fn classify_post_review_lane_blocks_stale_review_handoff_head_without_lineage_proof() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let marker_head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let current_head_oid = String::from("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&marker_head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(current_head_oid.clone()),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				"x/pubfi-pub-101",
				&current_head_oid,
				None,
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "review_handoff_lineage_check_failed");
}

#[test]
fn classify_post_review_lane_blocks_when_pull_request_head_differs_from_worktree_head() {
	let (_temp_dir, config, _workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let branch_name = worktree.branch_name.clone();
	let worktree_path = worktree.path.clone();
	let current_head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_head_oid = String::from("feedfacefeedfacefeedfacefeedfacefeedface");
	let marker_head_oid = current_head_oid.clone();

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&branch_name,
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees(config.service_id())
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			&branch_name,
			"https://github.com/hack-ink/decodex/pull/174",
			&marker_head_oid,
		)),
		local_branch_name: Some(branch_name.clone()),
		local_head_oid: Some(current_head_oid),
	};
	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(
			tests::sample_pull_request_review_state(
				"https://github.com/hack-ink/decodex/pull/174",
				&branch_name,
				&pr_head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("SUCCESS"),
				0,
			),
		)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Block);
	assert_eq!(classification.reason, "pull_request_head_mismatch");
}

#[test]
fn classify_post_review_lane_waits_when_external_review_request_is_still_pending() {
	for review_decision in [None, Some("APPROVED")] {
		let temp_dir = TempDir::new().expect("temp dir should exist");
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = tests::sample_issue("In Review", &[]);
		let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
		let worktree_path = temp_dir.path().join("lane");

		fs::create_dir_all(&worktree_path).expect("worktree path should exist");

		state_store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-101",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		let worktree = state_store
			.list_worktrees("pubfi")
			.expect("worktree list should succeed")
			.into_iter()
			.next()
			.expect("worktree should exist");
		let snapshot = PostReviewLaneSnapshot {
			issue,
			worktree,
			review_handoff: Some(tests::sample_review_handoff_marker(
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/174",
				&head_oid,
			)),
			local_branch_name: Some(String::from("x/pubfi-pub-101")),
			local_head_oid: Some(head_oid.clone()),
		};

		tests::seed_review_orchestration_marker(
			&state_store,
			TEST_SERVICE_ID,
			&snapshot.issue.id,
			&tests::sample_review_orchestration_marker(
				"x/pubfi-pub-101",
				"https://github.com/hack-ink/decodex/pull/174",
				&head_oid,
				"waiting_for_ack",
				0,
			),
		);

		let mut review_state = tests::sample_pull_request_review_state_with_pending_requests(
			"https://github.com/hack-ink/decodex/pull/174",
			"x/pubfi-pub-101",
			&head_oid,
			review_decision,
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
			1,
		);

		tests::add_external_review_ack(&mut review_state);

		let classification = orchestrator::classify_post_review_lane(
			&snapshot,
			&state_store,
			&tests::sample_workflow(),
			&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
		)
		.expect("classification should succeed");

		assert_eq!(classification.decision, PostReviewLaneDecision::WaitForReview);
		assert_eq!(classification.reason, "external_review_result_pending");
	}
}

#[test]
fn classify_post_review_lane_continues_when_pull_request_is_already_merged() {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};
	let mut review_state = tests::sample_pull_request_review_state(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		Some("APPROVED"),
		"MERGEABLE",
		"CLEAN",
		Some("SUCCESS"),
		0,
	);

	review_state.state = String::from("MERGED");

	let classification = orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed");

	assert_eq!(classification.decision, PostReviewLaneDecision::Continue);
	assert_eq!(classification.reason, "pull_request_merged_closeout_pending");
}

#[test]
fn classify_post_review_lane_repairs_unmergeable_pr_before_review_waits() {
	let cases = [
		(
			"conflict before review required wait",
			Some("REVIEW_REQUIRED"),
			"CONFLICTING",
			"DIRTY",
			0,
			"pull_request_merge_conflict",
		),
		(
			"conflict before pending review wait",
			None,
			"CONFLICTING",
			"DIRTY",
			1,
			"pull_request_merge_conflict",
		),
		(
			"behind before review required wait",
			Some("REVIEW_REQUIRED"),
			"MERGEABLE",
			"BEHIND",
			0,
			"pull_request_branch_behind_base",
		),
		(
			"behind before pending review wait",
			None,
			"MERGEABLE",
			"BEHIND",
			1,
			"pull_request_branch_behind_base",
		),
	];

	for (case_name, review_decision, mergeable, merge_state, pending_requests, reason) in cases {
		let classification = classify_post_review_lane_with_pr_state(
			review_decision,
			mergeable,
			merge_state,
			Some("SUCCESS"),
			pending_requests,
		);

		assert_eq!(
			classification.decision,
			PostReviewLaneDecision::NeedsReviewRepair,
			"{case_name}"
		);
		assert_eq!(classification.reason, reason, "{case_name}");
	}
}

fn classify_post_review_lane_with_pr_state(
	review_decision: Option<&str>,
	mergeable: &str,
	merge_state: &str,
	status_check_state: Option<&str>,
	pending_review_requests: usize,
) -> PostReviewLaneClassification {
	let temp_dir = TempDir::new().expect("temp dir should exist");
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("In Review", &[]);
	let head_oid = String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6");
	let worktree_path = temp_dir.path().join("lane");

	fs::create_dir_all(&worktree_path).expect("worktree path should exist");

	state_store
		.upsert_worktree(
			"pubfi",
			&issue.id,
			"x/pubfi-pub-101",
			&worktree_path.display().to_string(),
		)
		.expect("worktree should record");

	let worktree = state_store
		.list_worktrees("pubfi")
		.expect("worktree list should succeed")
		.into_iter()
		.next()
		.expect("worktree should exist");
	let snapshot = PostReviewLaneSnapshot {
		issue,
		worktree,
		review_handoff: Some(tests::sample_review_handoff_marker(
			"x/pubfi-pub-101",
			"https://github.com/hack-ink/decodex/pull/174",
			&head_oid,
		)),
		local_branch_name: Some(String::from("x/pubfi-pub-101")),
		local_head_oid: Some(head_oid.clone()),
	};
	let review_state = tests::sample_pull_request_review_state_with_pending_requests(
		"https://github.com/hack-ink/decodex/pull/174",
		"x/pubfi-pub-101",
		&head_oid,
		review_decision,
		mergeable,
		merge_state,
		status_check_state,
		0,
		pending_review_requests,
	);

	orchestrator::classify_post_review_lane(
		&snapshot,
		&state_store,
		&tests::sample_workflow(),
		&FakePullRequestReviewStateInspector::new(vec![Ok(review_state)]),
	)
	.expect("classification should succeed")
}
