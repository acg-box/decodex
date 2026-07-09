mod authority_boundary;
mod handoff_head;
mod merge_readiness;
mod request_and_findings;

use std::{
	fs,
	path::Path,
	process::{Command, Stdio},
};

use tempfile::TempDir;

use crate::{
	orchestrator::{
		self, AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput,
		AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
		AuthorityDecisionRequestInput, PostReviewLaneClassification, PostReviewLaneSnapshot,
		StateStore, tests,
		tests::{FakePullRequestReviewStateInspector, TEST_SERVICE_ID},
	},
	state::ReviewPolicyCheckpointInput,
	tracker::TrackerIssue,
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

pub(super) fn record_policy_authority_boundary(
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

pub(super) fn record_clean_review_checkpoint_for_head(
	state_store: &StateStore,
	issue_id: &str,
	head_oid: &str,
) {
	state_store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: TEST_SERVICE_ID,
			issue_id,
			run_id: "run-review",
			attempt_number: 2,
			phase: "handoff",
			review_level: "standard",
			status: "clean",
			head_sha: head_oid,
			nonclean_rounds: 0,
			details_json: r#"{"accepted_findings":[],"rejected_findings":[]}"#,
		})
		.expect("clean review checkpoint should persist");
}

pub(super) fn initialize_empty_git_worktree(worktree_path: &Path) {
	let status = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.arg("init")
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.status()
		.expect("git init should run");

	assert!(status.success(), "git init should succeed");
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
		lifecycle_record: Some(tests::sample_review_lifecycle_record(
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
