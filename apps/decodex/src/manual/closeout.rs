use std::{
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
};

use color_eyre::{Report, eyre::WrapErr};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config,
	config::ServiceConfig,
	default_branch_sync, github, orchestrator,
	prelude::{Result, eyre},
	runtime,
	state::StateStore,
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowDocument,
	worktree::{self, WorktreeManager},
};

use super::{
	MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH, ManualAuthority, ManualLandCloseoutMarkerRecord,
	ManualLandContext, ManualLandLedgerContext, PreparedCloseout,
};

pub(super) fn finalize_land_closeout(
	context: &ManualLandContext,
	merge_commit: &str,
	default_branch: &str,
	landed_change_record: &str,
) -> Result<()> {
	let state_store = if context.prepared_closeout.is_some() {
		Some(runtime::open_runtime_store()?)
	} else {
		None
	};
	let worktree_path_for_event = manual_land_relative_worktree_path(context);

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue closeout requires a retained review handoff marker.")
		})?;
		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		apply_closeout(
			&context.cwd,
			&prepared_closeout.tracker,
			&prepared_closeout.completed_state,
			&ledger,
			landed_change_record,
		)?;
	}

	default_branch_sync::sync_repo_root_default_branch(
		&context.canonical_repo_root,
		default_branch,
		Some(context.default_branch_git_credentials()),
	)?;

	if context.prepared_closeout.is_none()
		&& !manual_land_closeout_matches(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)? {
		write_manual_land_closeout_marker(
			&context.cwd,
			&context.pr_url,
			merge_commit,
			&context.current_branch,
			landed_change_record,
		)?;
	}

	cleanup_manual_land_lane_checkout(context)?;

	if let Some(prepared_closeout) = context.prepared_closeout.as_ref() {
		let state_store = state_store
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Manual closeout state store was not opened."))?;
		let handoff = context.review_handoff.as_ref().ok_or_else(|| {
			eyre::eyre!("`decodex land` issue cleanup requires a retained review handoff marker.")
		})?;

		clear_manual_closeout_runtime_state(
			state_store,
			&prepared_closeout.issue.id,
			handoff.run_id(),
		)?;
		clear_manual_closeout_issue_scope(
			&prepared_closeout.tracker,
			&prepared_closeout.issue,
			&prepared_closeout.service_id,
			&prepared_closeout.needs_attention_label,
		)?;

		let ledger = ManualLandLedgerContext {
			service_id: &prepared_closeout.service_id,
			issue: &prepared_closeout.issue,
			state_store,
			handoff,
			pr_url: &context.pr_url,
			merge_commit,
			branch_name: &context.current_branch,
			worktree_path: &worktree_path_for_event,
			completed_state: &prepared_closeout.completed_state,
			default_branch,
			privacy_classifier: &context.public_projection_privacy_classifier,
		};

		write_manual_land_cleanup_complete_event(&prepared_closeout.tracker, &ledger)?;
	}

	Ok(())
}

pub(super) fn manual_land_relative_worktree_path(context: &ManualLandContext) -> String {
	if let Ok(relative_path) = context.worktree_root.strip_prefix(&context.canonical_repo_root) {
		if relative_path.as_os_str().is_empty() {
			return String::from(".");
		}

		return relative_path.display().to_string();
	}
	if let Some(root_name) = context.project_worktree_root.file_name()
		&& let Ok(relative_path) =
			context.worktree_root.strip_prefix(&context.project_worktree_root)
	{
		return Path::new(root_name).join(relative_path).display().to_string();
	}

	context.worktree_root.file_name().map_or_else(
		|| context.worktree_root.display().to_string(),
		|path| path.to_string_lossy().into_owned(),
	)
}

pub(super) fn cleanup_manual_land_lane_checkout(context: &ManualLandContext) -> Result<()> {
	let worktree_manager = WorktreeManager::new(
		context.service_id.as_str(),
		&context.canonical_repo_root,
		&context.project_worktree_root,
	);

	github::delete_pull_request_head_branch_if_present(
		&context.canonical_repo_root,
		&context.pr_url,
		&context.current_branch,
		&context.github_token,
		context.github_command_path.as_deref(),
	)?;
	orchestrator::detach_worktree_head_from_branch_if_checked_out(
		&context.worktree_root,
		&context.current_branch,
	)?;
	orchestrator::delete_local_branch_if_present(
		&context.canonical_repo_root,
		&context.current_branch,
	)?;

	if let Some(workflow) = context.workflow.as_ref() {
		worktree_manager.remove_worktree_path_with_hooks(
			manual_land_cleanup_identifier(&context.authority, &context.current_branch),
			&context.current_branch,
			&context.worktree_root,
			workflow.frontmatter().execution().workspace_hooks(),
		)?;
	} else {
		worktree_manager.remove_worktree_path(&context.worktree_root)?;
	}

	ensure_manual_land_left_no_merged_worktree_cleanup_debt(context)?;

	Ok(())
}

pub(super) fn ensure_manual_land_left_no_merged_worktree_cleanup_debt(
	context: &ManualLandContext,
) -> Result<()> {
	let debts = worktree::merged_worktree_cleanup_debts(
		&context.canonical_repo_root,
		&context.project_worktree_root,
		&context.repository.default_branch,
	)?;

	if debts.is_empty() {
		return Ok(());
	}

	let details = debts
		.iter()
		.map(|debt| {
			format!(
				"{} on {} ({})",
				debt.path.display(),
				debt.branch_name,
				if debt.cleanliness.is_dirty() { "dirty" } else { "clean" }
			)
		})
		.collect::<Vec<_>>()
		.join(", ");

	eyre::bail!(
		"`decodex land` completed the merge but post-land worktree cleanup debt remains under `{}`: {details}. Remove or salvage those worktrees before continuing automation.",
		context.project_worktree_root.display()
	);
}

pub(super) fn manual_land_cleanup_identifier<'a>(
	authority: &'a ManualAuthority,
	current_branch: &'a str,
) -> &'a str {
	authority.issue_identifier().unwrap_or(current_branch)
}

pub(super) fn prepare_closeout(
	config: &ServiceConfig,
	workflow: WorkflowDocument,
	authority: &str,
) -> Result<PreparedCloseout> {
	let tracker_policy = workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state().to_owned();
	let needs_attention_label = tracker_policy.needs_attention_label().to_owned();
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;
	let issue = tracker
		.get_issue_by_identifier(&authority.to_ascii_uppercase())?
		.ok_or_else(|| eyre::eyre!("Tracker does not contain issue `{authority}`."))?;

	ensure_manual_closeout_issue_scope(&tracker, &issue, config.service_id())?;

	Ok(PreparedCloseout {
		tracker,
		issue,
		completed_state,
		service_id: config.service_id().to_owned(),
		needs_attention_label,
	})
}

pub(super) fn ensure_manual_land_checkout_is_managed_lane(
	checkout_root: &Path,
	project_worktree_root: &Path,
	issue_identifier: &str,
) -> Result<()> {
	let canonical_checkout = fs::canonicalize(checkout_root).wrap_err_with(|| {
		format!("Failed to canonicalize current lane checkout `{}`.", checkout_root.display())
	})?;
	let canonical_worktree_root = fs::canonicalize(project_worktree_root).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			project_worktree_root.display()
		)
	})?;

	if canonical_checkout.starts_with(&canonical_worktree_root)
		&& canonical_checkout != canonical_worktree_root
	{
		return Ok(());
	}

	eyre::bail!(
		"`decodex land` for issue `{issue_identifier}` must run from a managed lane under worktree_root `{}` so successful land can clean up the worktree and branch.",
		project_worktree_root.display()
	);
}

pub(super) fn ensure_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let active_label = tracker::automation_active_label(service_id);

	if tracker::issue_has_label_with_server_confirmation(tracker, issue, &active_label)? {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is not owned by service `{service_id}`; `decodex land` requires label `{active_label}`.",
		issue.identifier
	);
}

pub(super) fn apply_closeout<T>(
	checkout_root: &Path,
	tracker: &T,
	completed_state: &str,
	ledger: &ManualLandLedgerContext<'_>,
	landed_change_record: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if ledger.issue.state.name != completed_state {
		let state_id = ledger.issue.state_id_for_name(completed_state).ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` does not expose tracker state `{}` on its team.",
				ledger.issue.identifier,
				completed_state
			)
		})?;

		tracker.update_issue_state(ledger.issue.id.as_str(), state_id)?;
	}
	if !manual_land_closeout_matches(
		checkout_root,
		ledger.pr_url,
		ledger.merge_commit,
		ledger.branch_name,
		landed_change_record,
	)? {
		tracker::create_public_comment(
			tracker,
			ledger.issue.id.as_str(),
			format!(
				"decodex land completed\n\n- pr_url: `{}`\n- merge_commit: `{}`\n- branch: `{}`\n- landed_change: `{landed_change_record}`",
				ledger.pr_url, ledger.merge_commit, ledger.branch_name
			)
			.as_str(),
		)?;

		write_manual_land_closeout_marker(
			checkout_root,
			ledger.pr_url,
			ledger.merge_commit,
			ledger.branch_name,
			landed_change_record,
		)?;
	}

	write_manual_land_landed_and_closeout_events(tracker, ledger)?;
	succeed_manual_land_handoff_attempt(
		ledger.state_store,
		&ledger.issue.id,
		ledger.handoff.run_id(),
	)?;

	Ok(())
}

pub(super) fn write_manual_land_landed_and_closeout_events<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let landed = manual_land_landed_event(ledger);
	let closeout = manual_land_closeout_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &landed)?;

	write_manual_land_lifecycle_event(tracker, ledger, &closeout)
}

pub(super) fn write_manual_land_cleanup_complete_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let cleanup_complete = manual_land_cleanup_complete_event(ledger);

	write_manual_land_lifecycle_event(tracker, ledger, &cleanup_complete)
}

pub(super) fn write_manual_land_lifecycle_event<T>(
	tracker: &T,
	ledger: &ManualLandLedgerContext<'_>,
	record: &LinearExecutionEventRecord,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let retry_budget_attempt_count =
		ledger.state_store.retry_budget_attempt_count(&ledger.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body =
		records::render_linear_execution_event_comment_body(record, retry_budget_attempt_count);
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, record, ledger.privacy_classifier)?;

	if ledger.state_store.record_linear_execution_event(&projection.record)?
		&& let Err(error) =
			tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
				tracker,
				&ledger.issue.id,
				&projection,
			) {
		ledger.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(())
}

pub(super) fn manual_land_landed_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.pr_url,
		ledger.handoff.pr_head_oid(),
		ledger.merge_commit,
		"manual_land_landed",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"landed",
		manual_land_ordered_event_timestamp(-2),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.pr_head_sha = Some(ledger.handoff.pr_head_oid().to_owned());
	record.pr_base_ref =
		Some(ledger.handoff.target_base_ref_name().unwrap_or(ledger.default_branch).to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.summary =
		Some(format!("Manual land merged {} for {}.", ledger.pr_url, ledger.issue.identifier));

	record
}

pub(super) fn manual_land_closeout_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor =
		records::stable_event_anchor(&[ledger.pr_url, ledger.merge_commit, "manual_land_closeout"]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"closeout",
		manual_land_ordered_event_timestamp(-1),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());
	record.validation_result = Some(String::from("passed"));
	record.target_state = Some(ledger.completed_state.to_owned());
	record.summary = Some(format!(
		"Manual land closed out {} after merge {}.",
		ledger.issue.identifier, ledger.merge_commit
	));

	record
}

pub(super) fn manual_land_cleanup_complete_event(
	ledger: &ManualLandLedgerContext<'_>,
) -> LinearExecutionEventRecord {
	let anchor = records::stable_event_anchor(&[
		ledger.branch_name,
		ledger.merge_commit,
		"manual_land_cleanup_complete",
	]);
	let mut record = LinearExecutionEventRecord::new(
		manual_land_lifecycle_identity(ledger),
		"cleanup_complete",
		manual_land_ordered_event_timestamp(0),
		&anchor,
	);

	record.branch = Some(ledger.branch_name.to_owned());
	record.worktree_path = Some(ledger.worktree_path.to_owned());
	record.cleanup_status = Some(String::from("completed"));
	record.summary = Some(String::from("Manual land cleaned up the retained lane."));
	record.pr_url = Some(ledger.pr_url.to_owned());
	record.commit_sha = Some(ledger.merge_commit.to_owned());

	record
}

pub(super) fn manual_land_lifecycle_identity<'a>(
	ledger: &'a ManualLandLedgerContext<'_>,
) -> LinearExecutionEventIdentity<'a> {
	LinearExecutionEventIdentity {
		service_id: ledger.service_id,
		issue_id: &ledger.issue.id,
		issue_identifier: &ledger.issue.identifier,
		run_id: ledger.handoff.run_id(),
		attempt_number: ledger.handoff.attempt_number(),
	}
}

pub(super) fn manual_land_ordered_event_timestamp(offset_seconds: i64) -> String {
	(OffsetDateTime::now_utc() + time::Duration::seconds(offset_seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
}

pub(super) fn clear_manual_closeout_issue_scope<T>(
	tracker: &T,
	issue: &TrackerIssue,
	service_id: &str,
	needs_attention_label: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let closeout_labels = [
		tracker::automation_active_label(service_id),
		tracker::automation_queue_label(service_id),
		needs_attention_label.to_owned(),
	];

	for label_name in closeout_labels {
		clear_manual_closeout_issue_label(tracker, issue, &label_name)?;
	}

	Ok(())
}

pub(super) fn clear_manual_closeout_issue_label<T>(
	tracker: &T,
	issue: &TrackerIssue,
	label_name: &str,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if let Err(error) = tracker::set_issue_label_presence(tracker, issue, label_name, false)
		&& !linear_label_not_on_issue_error(&error)
	{
		return Err(error);
	}

	Ok(())
}

pub(super) fn clear_manual_closeout_runtime_state(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	state_store.succeed_running_run_attempts_for_issue(issue_id).wrap_err_with(|| {
		format!("Failed to finalize running runtime attempts for issue `{issue_id}`.")
	})?;

	succeed_manual_land_handoff_attempt(state_store, issue_id, handoff_run_id)?;

	state_store
		.clear_lease(issue_id)
		.wrap_err_with(|| format!("Failed to clear runtime lease for issue `{issue_id}`."))?;
	state_store.clear_worktree(issue_id).wrap_err_with(|| {
		format!("Failed to clear runtime worktree state for issue `{issue_id}`.")
	})?;

	Ok(())
}

pub(super) fn succeed_manual_land_handoff_attempt(
	state_store: &StateStore,
	issue_id: &str,
	handoff_run_id: &str,
) -> Result<()> {
	let Some(attempt) = state_store.run_attempt(handoff_run_id)? else {
		return Ok(());
	};

	if attempt.issue_id() != issue_id {
		eyre::bail!(
			"Manual land handoff run `{handoff_run_id}` belongs to issue `{}`, not `{issue_id}`.",
			attempt.issue_id()
		);
	}
	if attempt.status() != "succeeded" {
		state_store.update_run_status(handoff_run_id, "succeeded")?;
	}

	Ok(())
}

pub(super) fn linear_label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
}

pub(super) fn manual_land_closeout_marker_path(checkout_root: &Path) -> Result<PathBuf> {
	let Some(git_dir) = config::git_dir_for_checkout(checkout_root)? else {
		eyre::bail!(
			"Current checkout `{}` does not expose a Git administrative directory.",
			checkout_root.display()
		);
	};

	Ok(git_dir.join(MANUAL_LAND_CLOSEOUT_MARKER_GIT_PATH))
}

pub(super) fn manual_land_closeout_matches(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<bool> {
	let Some(marker) = read_manual_land_closeout_marker(checkout_root)? else {
		return Ok(false);
	};

	Ok(marker.pr_url.as_deref() == Some(pr_url)
		&& marker.merge_commit.as_deref() == Some(merge_commit)
		&& marker.branch_name.as_deref() == Some(branch_name)
		&& marker.landed_change.as_deref() == Some(landed_change_record))
}

pub(super) fn read_manual_land_closeout_marker(
	checkout_root: &Path,
) -> Result<Option<ManualLandCloseoutMarkerRecord>> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(marker_body) => marker_body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => {
			return Err(error).wrap_err_with(|| {
				format!("Failed to read manual land closeout marker `{}`.", marker_path.display())
			});
		},
	};
	let mut marker = ManualLandCloseoutMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"pr_url" => marker.pr_url = Some(value.to_owned()),
			"merge_commit" => marker.merge_commit = Some(value.to_owned()),
			"branch_name" => marker.branch_name = Some(value.to_owned()),
			"landed_change" => marker.landed_change = Some(value.to_owned()),
			_ => {},
		}
	}

	Ok(Some(marker))
}

pub(super) fn write_manual_land_closeout_marker(
	checkout_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	branch_name: &str,
	landed_change_record: &str,
) -> Result<()> {
	let marker_path = manual_land_closeout_marker_path(checkout_root)?;
	let Some(marker_dir) = marker_path.parent() else {
		eyre::bail!(
			"Manual land closeout marker path `{}` has no parent directory.",
			marker_path.display()
		);
	};

	fs::create_dir_all(marker_dir).wrap_err_with(|| {
		format!(
			"Failed to create manual land closeout marker directory `{}`.",
			marker_dir.display()
		)
	})?;
	fs::write(
		&marker_path,
		format!(
			"pr_url={pr_url}\nmerge_commit={merge_commit}\nbranch_name={branch_name}\nlanded_change={landed_change_record}\n"
		),
	)
	.wrap_err_with(|| {
		format!("Failed to write manual land closeout marker `{}`.", marker_path.display())
	})?;

	Ok(())
}
