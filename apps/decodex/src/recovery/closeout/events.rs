use crate::{
	recovery::{
		LEGACY_MANUAL_CLOSEOUT_ANCHOR, LEGACY_MANUAL_CLOSEOUT_EVENT,
		MERGED_CLOSEOUT_CLEANUP_ANCHOR, MERGED_CLOSEOUT_CLOSEOUT_ANCHOR,
		SUPERSEDED_CLOSEOUT_ANCHOR, SUPERSEDED_CLOSEOUT_CLEANUP_ANCHOR,
		closeout::{
			LegacyCloseoutValidation, MergedCloseoutValidation, SupersededCloseoutValidation,
		},
		context::RecoveryContext,
		events::{self},
		pull_request_inspection,
	},
	tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
};

pub(super) fn legacy_closeout_event(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = pull_request_inspection::landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.local_head_oid,
		&validation.merge_commit,
		LEGACY_MANUAL_CLOSEOUT_ANCHOR,
	]);
	let run_id = format!("legacy-closeout-{}", validation.issue.identifier.to_ascii_lowercase());
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &run_id,
			attempt_number: 1,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		events::current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.worktree.branch_name().to_owned());
	event.worktree_path = validation.worktree_path_for_event.clone();
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.local_head_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.validation_result = Some(String::from("passed"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.cleanup_status = Some(String::from("manual_audit_recorded"));
	event.summary = Some(format!(
		"Legacy manual closeout audit recorded for {} after merged PR {}.",
		validation.issue.identifier, pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("merge_commit={}", validation.merge_commit),
		format!("worktree_provenance={}", validation.worktree.provenance().source()),
		String::from("worktree_clean=true"),
	]);
	event.next_action = Some(String::from(
		"remove the local worktree only after preserving or discarding local-only changes intentionally",
	));

	event
}

pub(super) fn merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = pull_request_inspection::landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		pr_url,
		&validation.merge_commit,
		MERGED_CLOSEOUT_CLOSEOUT_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		events::current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.validation_result = Some(String::from("passed"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.summary = Some(format!(
		"Merged closeout recovery recorded for {} after PR {} was already merged.",
		validation.issue.identifier, pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.landing_state.head_ref_oid),
		format!("merge_commit={}", validation.merge_commit),
		String::from("origin_default_contains_merge_commit=true"),
	]);
	event.next_action = Some(String::from(
		"Decodex will record cleanup_complete for the already-merged retained lane.",
	));

	event
}

pub(super) fn merged_closeout_cleanup_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = pull_request_inspection::landing_url(&validation.landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		&validation.branch_name,
		&validation.worktree_path_for_event,
		&validation.merge_commit,
		MERGED_CLOSEOUT_CLEANUP_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		"cleanup_complete",
		events::timestamp_after_seconds(1),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(pr_url.to_owned());
	event.pr_head_sha = Some(validation.landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.merge_commit.clone());
	event.cleanup_status = Some(String::from("merged_closeout_reconciled"));
	event.target_state = Some(validation.issue.state.name.clone());
	event.summary = Some(format!(
		"Merged closeout recovery marked stale retained lane {} cleanup complete.",
		validation.issue.identifier
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("worktree_path={}", validation.worktree_path_for_event),
		String::from("linear_queue_active_attention_labels_absent=true"),
		String::from("retained_worktree_has_no_uncommitted_changes=true"),
	]);
	event.next_action = Some(String::from(
		"Decodex will close the obsolete PR, record lifecycle authority, and clear retained lane cleanup state.",
	));

	event
}

pub(super) fn superseded_closeout_event(
	context: &RecoveryContext,
	validation: &SupersededCloseoutValidation,
) -> LinearExecutionEventRecord {
	let obsolete_pr_url = pull_request_inspection::landing_url(&validation.obsolete_landing_state);
	let successor_pr_url =
		pull_request_inspection::landing_url(&validation.successor_landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		obsolete_pr_url,
		successor_pr_url,
		&validation.successor_merge_commit,
		SUPERSEDED_CLOSEOUT_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		LEGACY_MANUAL_CLOSEOUT_EVENT,
		events::current_timestamp(),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(obsolete_pr_url.to_owned());
	event.pr_head_sha = Some(validation.obsolete_landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.obsolete_landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.successor_merge_commit.clone());
	event.validation_result = Some(String::from("passed"));
	event.target_state =
		Some(context.workflow.frontmatter().tracker().resolved_completed_state().to_owned());
	event.summary = Some(format!(
		"Superseded closeout recorded for {} after successor issue {} landed PR {}.",
		validation.issue.identifier, validation.successor_issue.identifier, successor_pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("successor_issue={}", validation.successor_issue.identifier),
		format!("successor_issue_state={}", validation.successor_issue.state.name),
		format!("obsolete_pr_url={obsolete_pr_url}"),
		format!("obsolete_pr_head_sha={}", validation.obsolete_landing_state.head_ref_oid),
		format!("successor_pr_url={successor_pr_url}"),
		format!("successor_pr_head_sha={}", validation.successor_landing_state.head_ref_oid),
		format!("successor_merge_commit={}", validation.successor_merge_commit),
		String::from("obsolete_pr_has_no_unique_unlanded_patch=true"),
		String::from("retained_worktree_has_no_uncommitted_changes=true"),
	]);
	event.next_action = Some(String::from(
		"Decodex will close the obsolete PR and clear retained lane cleanup state.",
	));

	event
}

pub(super) fn superseded_closeout_cleanup_event(
	context: &RecoveryContext,
	validation: &SupersededCloseoutValidation,
) -> LinearExecutionEventRecord {
	let obsolete_pr_url = pull_request_inspection::landing_url(&validation.obsolete_landing_state);
	let successor_pr_url =
		pull_request_inspection::landing_url(&validation.successor_landing_state);
	let stable_anchor = records::stable_event_anchor(&[
		&validation.branch_name,
		&validation.worktree_path_for_event,
		&validation.successor_merge_commit,
		SUPERSEDED_CLOSEOUT_CLEANUP_ANCHOR,
	]);
	let mut event = LinearExecutionEventRecord::new(
		LinearExecutionEventIdentity {
			service_id: context.config.service_id(),
			issue_id: &validation.issue.id,
			issue_identifier: &validation.issue.identifier,
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
		},
		"cleanup_complete",
		events::timestamp_after_seconds(1),
		&stable_anchor,
	);

	event.branch = Some(validation.branch_name.clone());
	event.worktree_path = Some(validation.worktree_path_for_event.clone());
	event.pr_url = Some(obsolete_pr_url.to_owned());
	event.pr_head_sha = Some(validation.obsolete_landing_state.head_ref_oid.clone());
	event.pr_base_ref = Some(validation.obsolete_landing_state.base_ref_name.clone());
	event.commit_sha = Some(validation.successor_merge_commit.clone());
	event.cleanup_status = Some(String::from("superseded_closeout_reconciled"));
	event.target_state =
		Some(context.workflow.frontmatter().tracker().resolved_completed_state().to_owned());
	event.summary = Some(format!(
		"Superseded closeout marked retained lane {} cleanup complete after successor PR {}.",
		validation.issue.identifier, successor_pr_url
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.branch_name),
		format!("worktree_path={}", validation.worktree_path_for_event),
		format!("successor_issue={}", validation.successor_issue.identifier),
		String::from("obsolete_pr_closure_authorized=true"),
		String::from("retained_worktree_has_no_uncommitted_changes=true"),
	]);
	event.next_action = Some(String::from("No Decodex runtime action remains for this lane."));

	event
}
