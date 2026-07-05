use crate::{
	recovery::{
		self, AdoptValidation, REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_REBIND_EVENT,
		RebindValidation, RecoveryContext, events,
	},
	tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

pub(in crate::recovery) fn review_handoff_rebind_event(
	context: &RecoveryContext,
	validation: &RebindValidation,
	active_label_restored: bool,
) -> LinearExecutionEventRecord {
	let pr_url = recovery::landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.local_head_oid,
		REVIEW_HANDOFF_REBIND_EVENT,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		REVIEW_HANDOFF_REBIND_EVENT,
		events::current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.worktree.branch_name().to_owned());
	event.worktree_path = validation.worktree_path_for_event.clone();
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.local_head_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.local_head_oid.clone());
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(format!(
		"Explicit operator rebind {} for {}.",
		validation.mode.summary_action(),
		validation.issue.identifier,
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("existing_review_lifecycle_record={}", validation.mode.evidence_value()),
		format!("active_label_present={}", validation.active_label_present),
		format!("active_label_repair={active_label_restored}"),
		format!("needs_attention_label_repair={}", validation.clear_needs_attention_label),
	]);
	event.next_action = Some(String::from("continue retained post-review lifecycle"));

	event
}

pub(in crate::recovery) fn review_handoff_adopt_event(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	active_label_restored: bool,
) -> LinearExecutionEventRecord {
	let pr_url = recovery::landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.local_head_oid,
		REVIEW_HANDOFF_ADOPT_EVENT,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		REVIEW_HANDOFF_ADOPT_EVENT,
		events::current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = validation.worktree_path_for_event.clone();
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.local_head_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.local_head_oid.clone());
	event.validation_result = Some(String::from("passed"));
	event.summary = Some(format!(
		"Explicit operator manual takeover adopted review handoff for {}.",
		validation.issue.identifier,
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("active_label_present={}", validation.active_label_present),
		format!("active_label_restored={active_label_restored}"),
		String::from("manual_takeover_adopt=true"),
		format!(
			"existing_retained_worktree_mapping={}",
			validation.previous_worktree_mapping.is_some()
		),
		String::from("existing_review_lifecycle_record=false"),
	]);
	event.next_action = Some(String::from("continue retained post-review lifecycle"));

	event
}

pub(in crate::recovery) fn manual_adopt_run_id(
	issue_identifier: &str,
	attempt_number: i64,
	head_oid: &str,
) -> String {
	let normalized_issue = issue_identifier
		.chars()
		.map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
		.collect::<String>();
	let head_prefix = head_oid.chars().take(12).collect::<String>();

	format!("{normalized_issue}-manual-adopt-{attempt_number}-{head_prefix}")
}
