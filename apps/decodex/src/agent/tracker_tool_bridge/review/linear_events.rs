use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{self, PullRequestDetails, ReviewHandoffContext},
	state::{ReviewLifecycleHandoffInput, ReviewLifecycleRecord},
	tracker::{TrackerIssue, records},
};

pub(super) fn review_policy_stop_fingerprint(details_json: &str) -> Option<String> {
	serde_json::from_str::<Value>(details_json)
		.ok()?
		.get("finding_policy")?
		.get("stop_fingerprint")?
		.as_str()
		.map(str::to_owned)
}

pub(super) fn review_lifecycle_handoff_from_pull_request<'a>(
	review_context: &'a ReviewHandoffContext,
	pull_request: &'a PullRequestDetails,
) -> ReviewLifecycleHandoffInput<'a> {
	ReviewLifecycleHandoffInput {
		run_id: &review_context.run_id,
		attempt_number: review_context.attempt_number,
		branch_name: &review_context.branch_name,
		pr_url: &pull_request.url,
		base_ref_name: &pull_request.base_ref_name,
		head_ref_name: &pull_request.head_ref_name,
		head_sha: &pull_request.head_ref_oid,
	}
}

pub(super) fn review_lifecycle_handoff_lineage_matches(
	existing: &ReviewLifecycleRecord,
	input: &ReviewLifecycleHandoffInput<'_>,
) -> bool {
	existing.branch_name() == input.branch_name
		&& existing.pr_url() == input.pr_url
		&& existing.target_base_ref_name() == Some(input.base_ref_name)
		&& existing.pr_head_ref_name() == input.head_ref_name
		&& existing.pr_head_oid() == input.head_sha
}

pub(super) fn linear_execution_review_event(
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	pull_request: &PullRequestDetails,
	event_type: &str,
	terminal_path: &str,
	summary: &str,
) -> records::LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[&pull_request.url, &pull_request.head_ref_oid]);
	let mut record = records::LinearExecutionEventRecord::new(
		linear_execution_identity(issue, review_context),
		event_type,
		tracker_tool_bridge::current_timestamp(),
		&anchor,
	);

	record.branch = Some(review_context.branch_name.clone());
	record.worktree_path = Some(review_context.worktree_path.clone());
	record.pr_url = Some(pull_request.url.clone());
	record.pr_head_sha = Some(pull_request.head_ref_oid.clone());
	record.pr_base_ref = Some(pull_request.base_ref_name.clone());
	record.commit_sha = Some(pull_request.head_ref_oid.clone());
	record.validation_result = Some(String::from("passed"));
	record.summary = Some(summary.to_owned());
	record.terminal_path = Some(terminal_path.to_owned());
	record.verification = Some(vec![String::from("repo gate passed before tracker writeback")]);

	record
}

pub(super) fn linear_execution_closeout_event(
	issue: &TrackerIssue,
	review_context: &ReviewHandoffContext,
	pull_request: &PullRequestDetails,
	summary: &str,
) -> records::LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[&pull_request.url, &pull_request.head_ref_oid]);
	let mut record = records::LinearExecutionEventRecord::new(
		linear_execution_identity(issue, review_context),
		"closeout",
		tracker_tool_bridge::current_timestamp(),
		&anchor,
	);

	record.branch = Some(review_context.branch_name.clone());
	record.worktree_path = Some(review_context.worktree_path.clone());
	record.pr_url = Some(pull_request.url.clone());
	record.commit_sha = Some(pull_request.head_ref_oid.clone());
	record.summary = Some(summary.to_owned());
	record.validation_result = Some(String::from("passed"));

	record
}

fn linear_execution_identity<'a>(
	issue: &'a TrackerIssue,
	review_context: &'a ReviewHandoffContext,
) -> records::LinearExecutionEventIdentity<'a> {
	records::LinearExecutionEventIdentity {
		service_id: &review_context.service_id,
		issue_id: &issue.id,
		issue_identifier: &issue.identifier,
		run_id: &review_context.run_id,
		attempt_number: review_context.attempt_number,
	}
}
