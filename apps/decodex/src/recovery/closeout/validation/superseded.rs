use crate::{
	orchestrator,
	prelude::{Result, eyre},
	recovery::{
		closeout::{
			SupersededCloseoutValidation,
			validation::merged::{issue, pull_request},
		},
		context::RecoveryContext,
		process_liveness::{self, StaleActiveProcessLiveness},
		pull_request_inspection,
		requests::SupersededCloseoutRecoveryRequest,
		review_handoff,
	},
	state,
	tracker::{
		self, IssueTracker, TrackerIssue,
		records::{self, LinearExecutionEventRecord},
	},
};

pub(in crate::recovery) fn validate_superseded_closeout_request(
	context: &RecoveryContext,
	request: &SupersededCloseoutRecoveryRequest,
) -> Result<SupersededCloseoutValidation> {
	if request.issue.eq_ignore_ascii_case(&request.successor_issue) {
		eyre::bail!("Superseded issue and successor issue must be distinct.");
	}
	if request.pr_url.trim() == request.successor_pr_url.trim() {
		eyre::bail!("Superseded PR and successor PR must be distinct.");
	}

	let issue = review_handoff::load_issue_by_identifier(&context.tracker, &request.issue)?;
	let successor_issue =
		review_handoff::load_issue_by_identifier(&context.tracker, &request.successor_issue)?;
	let completed_state_id = validate_superseded_issue_context(context, &issue)?;
	validate_successor_issue_context(context, &successor_issue)?;
	validate_same_tracker_team(&issue, &successor_issue)?;

	let (obsolete_landing_state, default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.pr_url)?;
	validate_obsolete_pull_request(&obsolete_landing_state, &default_branch)?;

	let (successor_landing_state, successor_default_branch) =
		pull_request_inspection::inspect_project_pull_request(context, &request.successor_pr_url)?;
	if successor_default_branch != default_branch {
		eyre::bail!(
			"Successor PR default branch `{successor_default_branch}` does not match obsolete PR default branch `{default_branch}`."
		);
	}
	pull_request::validate_merged_closeout_pull_request(
		context,
		&successor_landing_state,
		&default_branch,
	)?;

	let successor_merge_commit =
		pull_request_inspection::inspect_project_pull_request_merge_commit(
			context,
			&request.successor_pr_url,
		)?;
	validate_successor_issue_pr_lineage(
		context,
		&context.tracker,
		&successor_issue,
		&successor_landing_state,
		&successor_merge_commit,
	)?;
	pull_request::ensure_merge_commit_reachable_from_remote_default_branch(
		context.config.repo_root(),
		&request.successor_pr_url,
		&successor_merge_commit,
		&default_branch,
	)?;
	pull_request::ensure_head_has_no_unique_patch_from_remote_default_branch(
		context.config.repo_root(),
		&obsolete_landing_state.head_ref_oid,
		&default_branch,
		"obsolete PR has no unique unlanded patch after the successor PR landed",
	)?;

	let worktree_mapping = issue::retained_worktree_mapping_for_issue(context, &issue)?
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` has no retained worktree mapping; superseded closeout requires the obsolete retained lane mapping.",
				issue.identifier
			)
		})?;
	let local_head = review_handoff::validate_retained_pr_worktree(
		&worktree_mapping,
		&obsolete_landing_state,
		"superseded closeout",
	)?;
	if local_head != obsolete_landing_state.head_ref_oid {
		eyre::bail!(
			"Retained worktree HEAD `{local_head}` does not match obsolete PR head `{}`.",
			obsolete_landing_state.head_ref_oid
		);
	}

	let worktree_path_for_event = review_handoff::relative_worktree_path_for_recovery(
		context,
		worktree_mapping.worktree_path(),
	)
	.unwrap_or_else(|| worktree_mapping.worktree_path().display().to_string());
	let (run_id, attempt_number) =
		if let Some(attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? {
			(attempt.run_id().to_owned(), attempt.attempt_number())
		} else {
			(format!("superseded-closeout-{}", issue.identifier.to_ascii_lowercase()), 1)
		};

	Ok(SupersededCloseoutValidation {
		issue,
		successor_issue,
		branch_name: worktree_mapping.branch_name().to_owned(),
		worktree_path_for_event,
		run_id,
		attempt_number,
		obsolete_landing_state,
		successor_landing_state,
		successor_merge_commit,
		completed_state_id,
	})
}

fn validate_same_tracker_team(issue: &TrackerIssue, successor_issue: &TrackerIssue) -> Result<()> {
	if issue.team.id == successor_issue.team.id {
		return Ok(());
	}

	eyre::bail!(
		"Superseded issue `{}` belongs to team `{}`, but successor issue `{}` belongs to team `{}`.",
		issue.identifier,
		issue.team.name,
		successor_issue.identifier,
		successor_issue.team.name
	)
}

fn validate_superseded_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<String> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}
	ensure_superseded_issue_terminalizable(context, issue)?;

	issue
		.team
		.states
		.iter()
		.find(|state| state.name == tracker_policy.resolved_completed_state())
		.map(|state| state.id.clone())
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` team has no completed state `{}`.",
				issue.identifier,
				tracker_policy.resolved_completed_state()
			)
		})
}

pub(in crate::recovery::closeout) fn ensure_superseded_issue_terminalizable(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<()> {
	ensure_superseded_issue_terminalizable_with_tracker(context, &context.tracker, issue)
}

fn ensure_superseded_issue_terminalizable_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
	issue: &TrackerIssue,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	ensure_superseded_issue_recovery_labels_absent_with_tracker(context, tracker, issue)?;
	ensure_issue_has_no_live_runtime_ownership(context, issue)
}

fn ensure_superseded_issue_recovery_labels_absent_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
	issue: &TrackerIssue,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let tracker_policy = context.workflow.frontmatter().tracker();

	for label in [
		tracker::automation_queue_label(context.config.service_id()),
		tracker::automation_active_label(context.config.service_id()),
		tracker_policy.needs_attention_label().to_owned(),
	] {
		if tracker::issue_has_label_with_server_confirmation(tracker, issue, &label)? {
			eyre::bail!(
				"Issue `{}` still has Linear label `{label}`; superseded closeout recovery requires queue, active, and needs-attention labels to be absent.",
				issue.identifier
			);
		}
	}

	Ok(())
}

fn ensure_issue_has_no_live_runtime_ownership(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<()> {
	let retained_worktree_mapping = issue::retained_worktree_mapping_for_issue(context, issue)?;
	ensure_retained_worktree_marker_has_no_live_runtime_ownership(
		issue,
		retained_worktree_mapping.as_ref(),
	)?;

	for issue_key in issue_keys(issue) {
		if context
			.state_store
			.issue_has_active_shared_claim_read_only(context.config.service_id(), &issue_key)?
		{
			eyre::bail!(
				"Issue `{}` still has active runtime ownership for `{issue_key}`; superseded closeout recovery requires live ownership to be absent.",
				issue.identifier
			);
		}

		for attempt in context.state_store.list_run_attempts_for_issue(&issue_key)? {
			if !orchestrator::local_run_attempt_status_is_terminal(attempt.status()) {
				eyre::bail!(
					"Issue `{}` still has non-terminal {} run `{}` for `{issue_key}`; superseded closeout recovery requires live ownership to be absent.",
					issue.identifier,
					attempt.status(),
					attempt.run_id()
				);
			}
		}
	}

	Ok(())
}

fn ensure_retained_worktree_marker_has_no_live_runtime_ownership(
	issue: &TrackerIssue,
	retained_worktree_mapping: Option<&state::WorktreeMapping>,
) -> Result<()> {
	let Some(worktree_mapping) = retained_worktree_mapping else {
		return Ok(());
	};
	let Some(marker) = state::read_run_activity_marker_snapshot(worktree_mapping.worktree_path())?
	else {
		return Ok(());
	};

	if let Some(retry_kind) = marker.retry_kind() {
		eyre::bail!(
			"Issue `{}` still has retry-scheduled runtime ownership for retained worktree run `{}` attempt {} ({retry_kind}); superseded closeout recovery requires live ownership to be absent.",
			issue.identifier,
			marker.run_id(),
			marker.attempt_number()
		);
	}

	ensure_marker_has_no_live_runtime_evidence(issue, &marker)
}

fn ensure_marker_has_no_live_runtime_evidence(
	issue: &TrackerIssue,
	marker: &state::RunActivityMarker,
) -> Result<()> {
	let run_id = marker.run_id();
	let attempt_number = marker.attempt_number();
	let marker_liveness =
		process_liveness::stale_active_optional_marker_process_liveness(Some(marker));
	match marker_liveness {
		StaleActiveProcessLiveness::Alive => eyre::bail!(
			"Issue `{}` still has live retained process ownership for retained worktree run `{run_id}` attempt {attempt_number}; superseded closeout recovery requires live ownership to be absent.",
			issue.identifier
		),
		StaleActiveProcessLiveness::Unknown if marker.process_id().is_some() => eyre::bail!(
			"Issue `{}` still has unknown retained process liveness for retained worktree run `{run_id}` attempt {attempt_number}; superseded closeout recovery requires live ownership to be absent.",
			issue.identifier
		),
		StaleActiveProcessLiveness::Unknown | StaleActiveProcessLiveness::NotAlive => {},
	}

	if process_liveness::stale_active_marker_thread_active(marker) {
		eyre::bail!(
			"Issue `{}` still has live retained thread ownership for retained worktree run `{run_id}` attempt {attempt_number}; superseded closeout recovery requires live ownership to be absent.",
			issue.identifier
		);
	}

	if marker.last_activity_unix_epoch().is_some()
		|| marker.current_operation().is_some()
		|| marker.last_progress_unix_epoch().is_some()
		|| marker.last_protocol_activity_unix_epoch().is_some()
		|| marker.event_count() > 0
		|| marker.last_event_type().is_some()
		|| marker.child_agent_activity().is_some()
		|| marker.protocol_activity().is_some()
	{
		eyre::bail!(
			"Issue `{}` still has retained activity marker ownership for retained worktree run `{run_id}` attempt {attempt_number}; superseded closeout recovery requires stale activity evidence to be cleared by explicit recovery before terminalization.",
			issue.identifier
		);
	}

	Ok(())
}

fn validate_successor_issue_pr_lineage<T>(
	context: &RecoveryContext,
	tracker: &T,
	successor_issue: &TrackerIssue,
	successor_landing_state: &crate::pull_request::PullRequestLandingState,
	successor_merge_commit: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let records = successor_issue_lifecycle_records(context, tracker, successor_issue)?;

	if records.iter().any(|record| {
		successor_issue_record_matches_pr_lineage(
			context,
			successor_issue,
			record,
			successor_landing_state,
			successor_merge_commit,
		)
	}) {
		return Ok(());
	}

	eyre::bail!(
		"Successor issue `{}` has no Decodex execution ledger record tying it to successor PR `{}` head `{}` and merge commit `{successor_merge_commit}`.",
		successor_issue.identifier,
		pull_request_inspection::landing_url(successor_landing_state),
		successor_landing_state.head_ref_oid
	)
}

fn successor_issue_lifecycle_records<T>(
	context: &RecoveryContext,
	tracker: &T,
	successor_issue: &TrackerIssue,
) -> Result<Vec<LinearExecutionEventRecord>>
where
	T: IssueTracker + ?Sized,
{
	let mut lineage_records = Vec::new();

	for issue_key in issue_keys(successor_issue) {
		lineage_records.extend(
			context
				.state_store
				.list_linear_execution_events(context.config.service_id(), &issue_key)?,
		);
	}

	lineage_records.extend(
		tracker
			.list_comments(&successor_issue.id)?
			.iter()
			.filter_map(|comment| records::parse_linear_execution_event_record(&comment.body)),
	);

	Ok(lineage_records
		.into_iter()
		.filter(|record| {
			record.service_id == context.config.service_id()
				&& (record.issue_id == successor_issue.id
					|| record.issue_identifier == successor_issue.identifier)
		})
		.collect())
}

fn successor_issue_record_matches_pr_lineage(
	context: &RecoveryContext,
	successor_issue: &TrackerIssue,
	record: &LinearExecutionEventRecord,
	successor_landing_state: &crate::pull_request::PullRequestLandingState,
	successor_merge_commit: &str,
) -> bool {
	record.service_id == context.config.service_id()
		&& (record.issue_id == successor_issue.id
			|| record.issue_identifier == successor_issue.identifier)
		&& record.pr_url.as_deref().map(str::trim)
			== Some(pull_request_inspection::landing_url(successor_landing_state))
		&& record.pr_head_sha.as_deref() == Some(successor_landing_state.head_ref_oid.as_str())
		&& record.commit_sha.as_deref() == Some(successor_merge_commit)
}

fn issue_keys(issue: &TrackerIssue) -> Vec<String> {
	let mut keys = vec![issue.id.clone()];

	if issue.identifier != issue.id {
		keys.push(issue.identifier.clone());
	}

	keys
}

fn validate_successor_issue_context(context: &RecoveryContext, issue: &TrackerIssue) -> Result<()> {
	let tracker_policy = context.workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();

	if issue.state.name != completed_state {
		eyre::bail!(
			"Successor issue `{}` is in `{}`, but superseded closeout requires `{completed_state}`.",
			issue.identifier,
			issue.state.name
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Successor issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	Ok(())
}

fn validate_obsolete_pull_request(
	landing_state: &crate::pull_request::PullRequestLandingState,
	default_branch: &str,
) -> Result<()> {
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Obsolete pull request `{}` targets `{}`, but configured default branch is `{default_branch}`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state == "MERGED" {
		eyre::bail!(
			"Obsolete pull request `{}` is already merged; use merged closeout recovery for same-PR lineage.",
			pull_request_inspection::landing_url(landing_state)
		);
	}
	if !matches!(landing_state.state.as_str(), "OPEN" | "CLOSED") {
		eyre::bail!(
			"Obsolete pull request `{}` is `{}`; superseded closeout requires `OPEN` or already `CLOSED`.",
			pull_request_inspection::landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Obsolete pull request `{}` does not expose the retained head branch required for superseded closeout.",
			pull_request_inspection::landing_url(landing_state)
		);
	}

	Ok(())
}

#[cfg(test)]
mod tests {
	use std::{cell::RefCell, fs};

	use tempfile::TempDir;
	use time::{OffsetDateTime, format_description::well_known::Rfc3339};

	use super::*;
	use crate::{
		config::ServiceConfig,
		recovery::{LEGACY_MANUAL_CLOSEOUT_EVENT, RecoveryRuntimeMutationPolicy},
		state::{ProtocolActivityMarker, ProtocolActivitySummary, StateStore},
		tracker::{
			TrackerComment, TrackerIssue, TrackerIssueBriefUpdate, TrackerIssueCreate,
			TrackerLabel, TrackerState, TrackerTeam,
			linear::LinearClient,
			records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
		},
		workflow::WorkflowDocument,
	};

	#[test]
	fn terminalizable_guard_rejects_queue_active_and_attention_labels() {
		let labels = [
			tracker::automation_queue_label("pubfi"),
			tracker::automation_active_label("pubfi"),
			String::from("decodex:needs-attention"),
		];

		for label in labels {
			let temp_dir = TempDir::new().expect("tempdir should create");
			let context =
				sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
			let issue = sample_issue_with_labels("Todo", &[label.clone()]);
			let tracker = TestTracker::with_issues(vec![issue.clone()]);
			let error =
				ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
					.expect_err("superseded closeout should reject ownership labels");

			assert!(
				error.to_string().contains(label.as_str()),
				"error should name rejected label `{label}`: {error}"
			);
		}
	}

	#[test]
	fn terminalizable_guard_rejects_live_runtime_ownership() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);

		context
			.state_store
			.upsert_lease(context.config.service_id(), &issue.id, "run-lease", "Todo")
			.expect("lease should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject active runtime lease");

		assert!(error.to_string().contains("active runtime ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_running_attempt_without_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);

		context
			.state_store
			.record_run_attempt("run-active", &issue.id, 1, "running")
			.expect("run should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject running attempts");

		assert!(error.to_string().contains("run-active"));
	}

	#[test]
	fn terminalizable_guard_rejects_continuation_pending_attempt_without_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);

		context
			.state_store
			.record_run_attempt("run-continuation", &issue.id, 1, "continuation_pending")
			.expect("run should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject continuation-owned attempts");

		assert!(error.to_string().contains("run-continuation"));
		assert!(error.to_string().contains("continuation_pending"));
	}

	#[test]
	fn terminalizable_guard_rejects_retry_scheduled_terminal_attempt() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-retry", &issue.id, 1, "failed")
			.expect("terminal run should persist");
		state::write_run_retry_schedule(
			&worktree_path,
			"run-retry",
			1,
			"failure",
			OffsetDateTime::now_utc().unix_timestamp() + 300,
		)
		.expect("retry schedule should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject retry-scheduled attempts");

		assert!(error.to_string().contains("run-retry"));
		assert!(error.to_string().contains("retry-scheduled runtime ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_retry_marker_without_attempt_row() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		state::write_run_retry_schedule(
			&worktree_path,
			"run-marker-retry",
			1,
			"failure",
			OffsetDateTime::now_utc().unix_timestamp() + 300,
		)
		.expect("retry schedule should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject marker-only retry ownership");

		assert!(error.to_string().contains("run-marker-retry"));
		assert!(error.to_string().contains("retry-scheduled runtime ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_mismatched_retry_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-latest", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");
		state::write_run_retry_schedule(
			&worktree_path,
			"run-older-retry",
			1,
			"failure",
			OffsetDateTime::now_utc().unix_timestamp() + 300,
		)
		.expect("retry schedule should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject mismatched retry ownership");

		assert!(error.to_string().contains("run-older-retry"));
		assert!(error.to_string().contains("retry-scheduled runtime ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_live_retained_marker_after_terminal_attempt() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-live-marker", &issue.id, 1, "failed")
			.expect("terminal run should persist");

		let protocol_activity = ProtocolActivitySummary {
			turn_status: Some(String::from("running")),
			waiting_reason: Some(String::from("model_execution")),
			rate_limit_status: None,
			recent_events: Vec::new(),
		};
		state::write_run_protocol_activity_marker(
			&worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-live-marker",
				attempt_number: 1,
				thread_id: Some("thread-live"),
				turn_id: Some("turn-live"),
				event_count: 1,
				last_event_type: "thread/turn/started",
				child_agent_activity: None,
				protocol_activity: Some(&protocol_activity),
			},
		)
		.expect("live protocol marker should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject live retained marker ownership");

		assert!(error.to_string().contains("run-live-marker"));
		assert!(error.to_string().contains("live retained"));
	}

	#[test]
	fn terminalizable_guard_rejects_live_retained_marker_without_attempt_row() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		write_live_protocol_marker(&worktree_path, "run-marker-only", 1);

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject marker-only live ownership");

		assert!(error.to_string().contains("run-marker-only"));
		assert!(error.to_string().contains("live retained"));
	}

	#[test]
	fn terminalizable_guard_rejects_mismatched_live_retained_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-latest", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");
		write_live_protocol_marker(&worktree_path, "run-older-live-marker", 1);

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err(
				"superseded closeout should reject mismatched live retained marker ownership",
			);

		assert!(error.to_string().contains("run-older-live-marker"));
		assert!(error.to_string().contains("live retained"));
	}

	#[test]
	fn terminalizable_guard_rejects_mismatched_dead_retained_activity_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-latest", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");
		fs::create_dir_all(&worktree_path).expect("worktree should exist");
		state::write_run_activity_marker_for_process(
			&worktree_path,
			"run-older-dead-marker",
			1,
			u32::MAX,
		)
		.expect("dead process marker should persist");
		write_live_protocol_marker(&worktree_path, "run-older-dead-marker", 1);

		let marker = state::read_run_activity_marker_snapshot(&worktree_path)
			.expect("marker should read")
			.expect("marker should exist");
		assert_eq!(
			process_liveness::stale_active_optional_marker_process_liveness(Some(&marker)),
			StaleActiveProcessLiveness::NotAlive
		);

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject stale retained activity evidence");

		assert!(error.to_string().contains("run-older-dead-marker"));
		assert!(error.to_string().contains("retained activity marker ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_mismatched_dead_activity_only_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-latest", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");
		fs::create_dir_all(&worktree_path).expect("worktree should exist");
		state::write_run_activity_marker_for_process(
			&worktree_path,
			"run-older-dead-activity-marker",
			1,
			u32::MAX,
		)
		.expect("dead activity-only marker should persist");

		let marker = state::read_run_activity_marker_snapshot(&worktree_path)
			.expect("marker should read")
			.expect("marker should exist");
		assert_eq!(
			process_liveness::stale_active_optional_marker_process_liveness(Some(&marker)),
			StaleActiveProcessLiveness::NotAlive
		);
		assert!(marker.last_activity_unix_epoch().is_some());
		assert!(marker.current_operation().is_none());
		assert!(marker.last_progress_unix_epoch().is_none());
		assert!(marker.last_protocol_activity_unix_epoch().is_none());
		assert_eq!(marker.event_count(), 0);
		assert!(marker.last_event_type().is_none());
		assert!(marker.child_agent_activity().is_none());
		assert!(marker.protocol_activity().is_none());

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject stale activity-only evidence");

		assert!(error.to_string().contains("run-older-dead-activity-marker"));
		assert!(error.to_string().contains("retained activity marker ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_mismatched_operation_only_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);
		let worktree_path = temp_dir.path().join("PUB-1704");

		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"y/pubfi-pub-1704",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should persist");
		context
			.state_store
			.record_run_attempt("run-latest", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");
		state::write_run_operation_marker_preserving_activity(
			&worktree_path,
			"run-older-operation-marker",
			1,
			state::RUN_OPERATION_AGENT_RUN,
		)
		.expect("operation-only marker should persist");

		let marker = state::read_run_activity_marker_snapshot(&worktree_path)
			.expect("marker should read")
			.expect("marker should exist");
		assert!(marker.last_activity_unix_epoch().is_none());
		assert_eq!(marker.current_operation(), Some(state::RUN_OPERATION_AGENT_RUN));

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject stale operation-only evidence");

		assert!(error.to_string().contains("run-older-operation-marker"));
		assert!(error.to_string().contains("retained activity marker ownership"));
	}

	#[test]
	fn terminalizable_guard_rejects_any_non_terminal_attempt_for_issue() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);

		context
			.state_store
			.record_run_attempt("run-older-active", &issue.id, 1, "running")
			.expect("older active run should persist");
		context
			.state_store
			.record_run_attempt("run-latest-terminal", &issue.id, 2, "succeeded")
			.expect("latest terminal run should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject any non-terminal attempt");

		assert!(error.to_string().contains("run-older-active"));
		assert!(error.to_string().contains("running"));
	}

	#[test]
	fn terminalizable_guard_rejects_non_terminal_identifier_attempt() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let issue = sample_issue("Todo");
		let tracker = TestTracker::with_issues(vec![issue.clone()]);

		context
			.state_store
			.record_run_attempt("run-identifier-active", &issue.identifier, 1, "running")
			.expect("identifier-keyed active run should persist");

		let error = ensure_superseded_issue_terminalizable_with_tracker(&context, &tracker, &issue)
			.expect_err("superseded closeout should reject identifier-keyed non-terminal attempt");

		assert!(error.to_string().contains("run-identifier-active"));
		assert!(error.to_string().contains(&issue.identifier));
	}

	#[test]
	fn successor_lineage_rejects_unrelated_completed_issue_and_pr() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let successor_issue = successor_issue();
		let mut successor_landing = sample_landing_state(
			"https://github.com/helixbox/pubfi-mono/pull/827",
			"y/pubfi-pub-1705",
			"0123456789abcdef0123456789abcdef01234567",
		);
		successor_landing.state = String::from("MERGED");
		let tracker = TestTracker::with_issues(vec![successor_issue.clone()]);

		let error = validate_successor_issue_pr_lineage(
			&context,
			&tracker,
			&successor_issue,
			&successor_landing,
			"1123456789abcdef0123456789abcdef01234567",
		)
		.expect_err("unrelated successor issue and PR should be rejected");

		assert!(error.to_string().contains("has no Decodex execution ledger record"));
	}

	#[test]
	fn successor_lineage_accepts_matching_successor_closeout_ledger() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
		let successor_issue = successor_issue();
		let mut successor_landing = sample_landing_state(
			"https://github.com/helixbox/pubfi-mono/pull/827",
			"y/pubfi-pub-1705",
			"0123456789abcdef0123456789abcdef01234567",
		);
		successor_landing.state = String::from("MERGED");
		let merge_commit = "1123456789abcdef0123456789abcdef01234567";
		let comment_body = successor_closeout_comment(
			&context,
			&successor_issue,
			&successor_landing,
			merge_commit,
		);
		let tracker = TestTracker::with_issues(vec![successor_issue.clone()]).with_comments(vec![
			TrackerComment { body: comment_body, created_at: current_timestamp() },
		]);

		validate_successor_issue_pr_lineage(
			&context,
			&tracker,
			&successor_issue,
			&successor_landing,
			merge_commit,
		)
		.expect("matching successor closeout ledger should prove lineage");
	}

	fn successor_issue() -> TrackerIssue {
		let mut issue = sample_issue("Done");

		issue.id = String::from("successor-issue-id");
		issue.identifier = String::from("PUB-1705");

		issue
	}

	fn write_live_protocol_marker(
		worktree_path: &std::path::Path,
		run_id: &str,
		attempt_number: i64,
	) {
		let protocol_activity = ProtocolActivitySummary {
			turn_status: Some(String::from("running")),
			waiting_reason: Some(String::from("model_execution")),
			rate_limit_status: None,
			recent_events: Vec::new(),
		};

		state::write_run_protocol_activity_marker(
			worktree_path,
			&ProtocolActivityMarker {
				run_id,
				attempt_number,
				thread_id: Some("thread-live"),
				turn_id: Some("turn-live"),
				event_count: 1,
				last_event_type: "thread/turn/started",
				child_agent_activity: None,
				protocol_activity: Some(&protocol_activity),
			},
		)
		.expect("live protocol marker should persist");
	}

	fn successor_closeout_comment(
		context: &RecoveryContext,
		successor_issue: &TrackerIssue,
		successor_landing: &crate::pull_request::PullRequestLandingState,
		merge_commit: &str,
	) -> String {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: context.config.service_id(),
				issue_id: &successor_issue.id,
				issue_identifier: &successor_issue.identifier,
				run_id: "pub-1705-attempt-1",
				attempt_number: 1,
			},
			LEGACY_MANUAL_CLOSEOUT_EVENT,
			current_timestamp(),
			"successor-closeout",
		);

		record.branch = Some(successor_landing.head_ref_name.clone());
		record.worktree_path = Some(String::from(".worktrees/PUB-1705"));
		record.pr_url = Some(successor_landing.url.clone());
		record.pr_head_sha = Some(successor_landing.head_ref_oid.clone());
		record.pr_base_ref = Some(successor_landing.base_ref_name.clone());
		record.commit_sha = Some(merge_commit.to_owned());
		record.validation_result = Some(String::from("passed"));
		record.target_state = Some(String::from("Done"));
		record.summary = Some(String::from("Successor closeout recorded."));

		records::append_structured_comment_record("successor closeout", &record)
			.expect("comment record should render")
	}

	fn sample_recovery_context(
		temp_dir: &TempDir,
		runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
	) -> RecoveryContext {
		let repo_root = temp_dir.path().join("repo");
		let config_path = temp_dir.path().join("project.toml");

		fs::create_dir_all(&repo_root).expect("repo root should exist");
		fs::write(
			&config_path,
			r#"
service_id = "pubfi"

[paths]
repo_root = "repo"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "HOME"
"#,
		)
		.expect("config should write");

		RecoveryContext {
			config: ServiceConfig::from_path(&config_path).expect("config should load"),
			workflow: sample_workflow(),
			state_store: StateStore::open_in_memory().expect("state store should open"),
			tracker: LinearClient::new(String::from("test-token"))
				.expect("linear client should build"),
			runtime_mutation_policy,
		}
	}

	fn sample_workflow() -> WorkflowDocument {
		WorkflowDocument::parse_markdown(
			r#"
+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 8
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = []
verify_commands = []

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++

Test workflow.
"#,
		)
		.expect("sample workflow should parse")
	}

	fn sample_landing_state(
		pr_url: &str,
		branch_name: &str,
		head_oid: &str,
	) -> crate::pull_request::PullRequestLandingState {
		crate::pull_request::PullRequestLandingState {
			url: pr_url.to_owned(),
			state: String::from("OPEN"),
			is_draft: false,
			review_decision: Some(String::from("APPROVED")),
			base_ref_name: String::from("main"),
			base_ref_oid: Some(String::from("base-sha")),
			pending_review_requests: 0,
			mergeable: String::from("MERGEABLE"),
			merge_state_status: String::from("CLEAN"),
			head_ref_name: branch_name.to_owned(),
			head_ref_oid: head_oid.to_owned(),
			status_check_rollup_state: Some(String::from("SUCCESS")),
			required_status_contexts: Vec::new(),
			unresolved_review_threads: 0,
		}
	}

	fn sample_issue(state_name: &str) -> TrackerIssue {
		let states = vec![
			TrackerState { id: String::from("state-todo"), name: String::from("Todo") },
			TrackerState { id: String::from("state-progress"), name: String::from("In Progress") },
			TrackerState { id: String::from("state-review"), name: String::from("In Review") },
			TrackerState { id: String::from("state-done"), name: String::from("Done") },
		];
		let state = states
			.iter()
			.find(|state| state.name == state_name)
			.expect("sample state should exist")
			.clone();

		TrackerIssue {
			id: String::from("issue-id"),
			identifier: String::from("PUB-1704"),
			#[cfg(test)]
			project_slug: None,
			title: String::from("Sample issue"),
			author: None,
			description: String::new(),
			priority: None,
			created_at: String::from("2026-06-09T00:00:00Z"),
			updated_at: String::from("2026-06-09T00:00:00Z"),
			state,
			team: TrackerTeam {
				id: String::from("team-id"),
				name: String::from("XY"),
				states,
				labels: Vec::new(),
			},
			labels_complete: true,
			labels: Vec::new(),
			blockers: Vec::new(),
		}
	}

	fn sample_issue_with_labels(state_name: &str, labels: &[String]) -> TrackerIssue {
		let mut issue = sample_issue(state_name);

		for label in labels {
			let tracker_label = TrackerLabel {
				id: format!("label-{}", label.replace(':', "-")),
				name: label.clone(),
			};

			issue.team.labels.push(tracker_label.clone());
			issue.labels.push(tracker_label);
		}

		issue
	}

	fn current_timestamp() -> String {
		OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
	}

	struct TestTracker {
		issues: Vec<TrackerIssue>,
		comments: Vec<TrackerComment>,
		state_updates: RefCell<Vec<(String, String)>>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
	}

	impl TestTracker {
		fn with_issues(issues: Vec<TrackerIssue>) -> Self {
			Self {
				issues,
				comments: Vec::new(),
				state_updates: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn with_comments(mut self, comments: Vec<TrackerComment>) -> Self {
			self.comments = comments;

			self
		}
	}

	impl IssueTracker for TestTracker {
		fn list_issues_with_label(&self, label_name: &str) -> Result<Vec<TrackerIssue>> {
			Ok(self.issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
		}

		fn find_team_label_id(&self, team_id: &str, label_name: &str) -> Result<Option<String>> {
			Ok(self
				.issues
				.iter()
				.find(|issue| issue.team.id == team_id)
				.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
		}

		fn get_issue_by_identifier(&self, issue_identifier: &str) -> Result<Option<TrackerIssue>> {
			Ok(self
				.issues
				.iter()
				.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
				.cloned())
		}

		fn refresh_issues(&self, issue_ids: &[String]) -> Result<Vec<TrackerIssue>> {
			Ok(self
				.issues
				.iter()
				.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
				.cloned()
				.collect())
		}

		fn list_comments(&self, _issue_id: &str) -> Result<Vec<TrackerComment>> {
			Ok(self.comments.clone())
		}

		fn update_issue_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
			self.state_updates.borrow_mut().push((issue_id.to_owned(), state_id.to_owned()));

			Ok(())
		}

		fn add_issue_labels(&self, _issue_id: &str, _label_ids: &[String]) -> Result<()> {
			Ok(())
		}

		fn remove_issue_labels(&self, issue_id: &str, label_ids: &[String]) -> Result<()> {
			self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> Result<()> {
			Ok(())
		}

		fn create_issue(&self, request: &TrackerIssueCreate) -> Result<TrackerIssue> {
			let _ = request;

			eyre::bail!("test tracker does not create issues")
		}

		fn update_issue_brief(
			&self,
			issue_id: &str,
			request: &TrackerIssueBriefUpdate,
		) -> Result<TrackerIssue> {
			let _ = (issue_id, request);

			eyre::bail!("test tracker does not update issue briefs")
		}
	}
}
