//! Explicit operator recovery surfaces for retained Decodex lanes.

use std::{
	collections::HashMap,
	env, fs,
	path::{Path, PathBuf},
	process::Command,
};

use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	github,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	runtime,
	state::{
		RUN_ACTIVITY_MARKER_FILE, ReviewHandoffMarker, ReviewOrchestrationMarker, RunAttempt,
		StateStore, WorktreeMapping,
	},
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowDocument,
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
const REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";

/// Read-only retained review handoff diagnostic request.
#[derive(Debug)]
pub(crate) struct ReviewHandoffDiagnoseRequest {
	/// Optional issue identifier to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit retained review handoff rebind request.
#[derive(Debug)]
pub(crate) struct ReviewHandoffRebindRequest {
	/// Issue identifier to repair.
	pub(crate) issue: String,
	/// Pull request URL to bind.
	pub(crate) pr_url: String,
	/// Validate without writing markers or tracker audit comments.
	pub(crate) dry_run: bool,
}

#[derive(Serialize)]
struct ReviewHandoffRecoveryReport {
	project_id: String,
	diagnostics: Vec<ReviewHandoffDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReviewHandoffDiagnostic {
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	issue_state: String,
	classification: String,
	reason: String,
	branch_name: String,
	worktree_path: String,
	local_branch_name: Option<String>,
	local_head_oid: Option<String>,
	worktree_clean: Option<bool>,
	existing_pr_url: Option<String>,
	active_label_present: Option<bool>,
	next_action: String,
}

struct RecoveryContext {
	config: ServiceConfig,
	workflow: WorkflowDocument,
	state_store: StateStore,
	tracker: LinearClient,
}

struct RebindValidation {
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	attempt: RunAttempt,
	landing_state: PullRequestLandingState,
	local_head_oid: String,
	worktree_path_for_event: Option<String>,
	active_label_present: bool,
}

/// Run a read-only retained review handoff diagnostic.
pub(crate) fn run_review_handoff_diagnose(
	config_path: Option<&Path>,
	request: &ReviewHandoffDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;
	let diagnostics = match request.issue.as_deref() {
		Some(issue_identifier) => vec![diagnose_issue(&context, issue_identifier)?],
		None => diagnose_all_retained_review_worktrees(&context)?,
	};
	let report = ReviewHandoffRecoveryReport {
		project_id: context.config.service_id().to_owned(),
		diagnostics,
	};

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_review_handoff_recovery_report(&report));
	}

	Ok(())
}

/// Run an explicit retained review handoff rebind.
pub(crate) fn run_review_handoff_rebind(
	config_path: Option<&Path>,
	request: &ReviewHandoffRebindRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;
	let validation = validate_rebind_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} active_label_present={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.active_label_present
		);

		return Ok(());
	}

	apply_review_handoff_rebind(&context, &validation)?;

	println!(
		"rebind ok: project={} issue={} branch={} pr={} head={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.local_head_oid
	);

	Ok(())
}

fn load_recovery_context(config_path: Option<&Path>) -> Result<RecoveryContext> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_recovery_config_path(config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	runtime::register_project_config(&state_store, &config_path, true)?;

	Ok(RecoveryContext { config, workflow, state_store, tracker })
}

fn resolve_recovery_config_path(
	config_path: Option<&Path>,
	state_store: &StateStore,
) -> Result<PathBuf> {
	if let Some(config_path) = config_path {
		return ServiceConfig::resolve_project_config_path(config_path);
	}

	runtime::registered_config_path_for_cwd(state_store, &env::current_dir()?)?.ok_or_else(|| {
		eyre::eyre!(
			"No Decodex project config found. Pass --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
		)
	})
}

fn diagnose_all_retained_review_worktrees(
	context: &RecoveryContext,
) -> Result<Vec<ReviewHandoffDiagnostic>> {
	let worktrees = context.state_store.list_worktrees(context.config.service_id())?;

	if worktrees.is_empty() {
		return Ok(Vec::new());
	}

	let issue_ids =
		worktrees.iter().map(|worktree| worktree.issue_id().to_owned()).collect::<Vec<_>>();
	let issues = context.tracker.refresh_issues(&issue_ids)?;
	let issues_by_id =
		issues.into_iter().map(|issue| (issue.id.clone(), issue)).collect::<HashMap<_, _>>();
	let success_state = context.workflow.frontmatter().tracker().success_state();
	let mut diagnostics = Vec::new();

	for worktree in worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id()).cloned() else {
			continue;
		};

		if issue.state.name != success_state {
			continue;
		}

		diagnostics.push(diagnose_issue_worktree(context, issue, worktree)?);
	}

	Ok(diagnostics)
}

fn diagnose_issue(
	context: &RecoveryContext,
	issue_identifier: &str,
) -> Result<ReviewHandoffDiagnostic> {
	let issue = load_issue_by_identifier(&context.tracker, issue_identifier)?;
	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	diagnose_issue_worktree(context, issue, worktree)
}

fn diagnose_issue_worktree(
	context: &RecoveryContext,
	issue: TrackerIssue,
	worktree: WorktreeMapping,
) -> Result<ReviewHandoffDiagnostic> {
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let local_branch_name = worktree_checkout_branch_name(worktree.worktree_path()).ok().flatten();
	let local_head_oid = worktree_head_oid(worktree.worktree_path()).ok().flatten();
	let worktree_clean = worktree_is_clean(worktree.worktree_path()).ok();
	let active_label_name = tracker::automation_active_label(context.config.service_id());
	let active_label_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		&issue,
		&active_label_name,
	)
	.ok();
	let missing_handoff = existing_handoff.is_none();

	Ok(ReviewHandoffDiagnostic {
		project_id: context.config.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		classification: diagnostic_classification(existing_handoff.as_ref()),
		reason: diagnostic_reason(existing_handoff.as_ref()),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: worktree.worktree_path().display().to_string(),
		local_branch_name,
		local_head_oid,
		worktree_clean,
		existing_pr_url: existing_handoff.map(|handoff| handoff.pr_url().to_owned()),
		active_label_present,
		next_action: diagnostic_next_action(
			context.config.service_id(),
			&issue.identifier,
			missing_handoff,
		),
	})
}

fn diagnostic_classification(existing_handoff: Option<&ReviewHandoffMarker>) -> String {
	if existing_handoff.is_some() {
		return String::from("review_handoff_bound");
	}

	String::from(ORPHANED_REVIEW_HANDOFF_CLASSIFICATION)
}

fn diagnostic_reason(existing_handoff: Option<&ReviewHandoffMarker>) -> String {
	if existing_handoff.is_some() {
		return String::from("review_handoff_record_present");
	}

	String::from(MISSING_HANDOFF_REASON)
}

fn diagnostic_next_action(
	service_id: &str,
	issue_identifier: &str,
	missing_handoff: bool,
) -> String {
	if !missing_handoff {
		return String::from("Continue the existing post-review lifecycle; no rebind is needed.");
	}

	format!(
		"Inspect PR lineage, ensure label `{}` is present, then run `decodex recover review-handoff rebind {} --pr <URL>` if the PR exactly matches this retained lane.",
		tracker::automation_active_label(service_id),
		issue_identifier
	)
}

fn render_review_handoff_recovery_report(report: &ReviewHandoffRecoveryReport) -> String {
	let mut output =
		format!("Review handoff recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  local_branch: {}\n  local_head: {}\n  worktree_clean: {}\n  existing_pr_url: {}\n  active_label_present: {}\n  next_action: {}\n",
			diagnostic.issue_identifier,
			diagnostic.issue_state,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.branch_name,
			diagnostic.worktree_path,
			optional_text(diagnostic.local_branch_name.as_deref()),
			optional_text(diagnostic.local_head_oid.as_deref()),
			diagnostic.worktree_clean.map_or_else(|| String::from("unknown"), |clean| clean.to_string()),
			optional_text(diagnostic.existing_pr_url.as_deref()),
			diagnostic.active_label_present.map_or_else(|| String::from("unknown"), |present| present.to_string()),
			diagnostic.next_action,
		));
	}

	output
}

fn optional_text(value: Option<&str>) -> &str {
	value.unwrap_or("none")
}

fn validate_rebind_request(
	context: &RecoveryContext,
	request: &ReviewHandoffRebindRequest,
) -> Result<RebindValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;
	let worktree = validate_rebind_issue_context(context, &issue)?;
	let attempt =
		context.state_store.latest_run_attempt_for_issue(&issue.id)?.ok_or_else(|| {
			eyre::eyre!("Issue `{}` has no recorded run attempt to rebind.", issue.identifier)
		})?;
	let landing_state = inspect_rebind_pull_request(context, &request.pr_url)?;
	let local_head_oid = validate_rebind_worktree(&worktree, &landing_state)?;
	let active_label_present = validate_rebind_tracker_labels(context, &issue)?;
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), worktree.worktree_path());

	Ok(RebindValidation {
		issue,
		worktree,
		attempt,
		landing_state,
		local_head_oid,
		worktree_path_for_event,
		active_label_present,
	})
}

fn load_issue_by_identifier<T>(tracker: &T, issue_identifier: &str) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	tracker
		.get_issue_by_identifier(issue_identifier)?
		.ok_or_else(|| eyre::eyre!("Tracker issue `{issue_identifier}` was not found."))
}

fn validate_rebind_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.state.name != tracker_policy.success_state() {
		eyre::bail!(
			"Issue `{}` is in `{}`, but review handoff rebind requires `{}`.",
			issue.identifier,
			issue.state.name,
			tracker_policy.success_state()
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}
	if issue.has_label(tracker_policy.needs_attention_label()) {
		eyre::bail!(
			"Issue `{}` has needs-attention label `{}`.",
			issue.identifier,
			tracker_policy.needs_attention_label()
		);
	}

	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;

	if let Some(existing_handoff) = existing_handoff {
		eyre::bail!(
			"Issue `{}` already has review handoff marker for branch `{}` and PR `{}`.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url()
		);
	}

	Ok(worktree)
}

fn inspect_rebind_pull_request(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<PullRequestLandingState> {
	let github_token = context.config.github().resolve_token()?;
	let repository = github::inspect_repository_context(context.config.repo_root(), &github_token)?;

	if !github::pull_request_matches_repository(pr_url, &repository)? {
		eyre::bail!(
			"Pull request `{}` does not belong to configured repository `{}/{}`.",
			pr_url,
			repository.owner,
			repository.name
		);
	}

	let landing_state = github::inspect_pull_request_landing_state(
		context.config.repo_root(),
		pr_url,
		&github_token,
	)?;

	if landing_state.base_ref_name != repository.default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			pr_url,
			landing_state.base_ref_name,
			repository.default_branch
		);
	}
	if landing_state.state != "OPEN" {
		eyre::bail!(
			"Pull request `{pr_url}` is `{}`; rebind requires `OPEN`.",
			landing_state.state
		);
	}
	if landing_state.is_draft {
		eyre::bail!("Pull request `{pr_url}` is still draft.");
	}

	Ok(landing_state)
}

fn validate_rebind_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	let local_branch = worktree_checkout_branch_name(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree is detached."))?;

	if local_branch != worktree.branch_name() {
		eyre::bail!(
			"Retained worktree branch is `{local_branch}`, but runtime mapping expects `{}`.",
			worktree.branch_name()
		);
	}
	if landing_state.head_ref_name != worktree.branch_name() {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			landing_url(landing_state),
			landing_state.head_ref_name,
			worktree.branch_name()
		);
	}
	if !worktree_is_clean(worktree.worktree_path())? {
		eyre::bail!(
			"Retained worktree `{}` has local changes; rebind requires a clean lane checkout.",
			worktree.worktree_path().display()
		);
	}

	let local_head = worktree_head_oid(worktree.worktree_path())?
		.ok_or_else(|| eyre::eyre!("Retained worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but retained worktree HEAD is `{local_head}`.",
			landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(local_head)
}

fn validate_rebind_tracker_labels(context: &RecoveryContext, issue: &TrackerIssue) -> Result<bool> {
	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;

	if !active_label_present {
		eyre::bail!(
			"Issue `{}` is missing active automation label `{active_label}`. Restore explicit lane ownership before rebind.",
			issue.identifier
		);
	}

	Ok(active_label_present)
}

fn apply_review_handoff_rebind(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<()> {
	let handoff_marker = ReviewHandoffMarker::new(
		validation.attempt.run_id(),
		validation.attempt.attempt_number(),
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.landing_state.base_ref_name.clone(),
		validation.landing_state.head_ref_name.clone(),
		validation.local_head_oid.clone(),
	);
	let orchestration_marker = ReviewOrchestrationMarker::new(
		validation.attempt.run_id(),
		validation.attempt.attempt_number(),
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.local_head_oid.clone(),
		REBOUND_ORCHESTRATION_PHASE,
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let event = review_handoff_rebind_event(context, validation);

	context.state_store.upsert_review_handoff_marker(
		context.config.service_id(),
		&validation.issue.id,
		&handoff_marker,
	)?;
	context.state_store.upsert_review_orchestration_marker(
		context.config.service_id(),
		&validation.issue.id,
		&orchestration_marker,
	)?;

	if let Err(error) = write_rebind_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		context.state_store.clear_review_markers_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		return Err(error);
	}

	Ok(())
}

fn review_handoff_rebind_event(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
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
			run_id: validation.attempt.run_id(),
			attempt_number: validation.attempt.attempt_number(),
		},
		REVIEW_HANDOFF_REBIND_EVENT,
		current_timestamp(),
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
		"Explicit operator rebind restored retained review handoff marker for {}.",
		validation.issue.identifier
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		String::from("existing_review_handoff_marker=absent"),
	]);
	event.next_action = Some(String::from("continue retained post-review lifecycle"));

	event
}

fn write_rebind_audit(
	context: &RecoveryContext,
	validation: &RebindValidation,
	event: &LinearExecutionEventRecord,
) -> Result<()> {
	let body = format!(
		"Decodex operator recovery: rebound retained review handoff marker for `{}` to `{}`. This does not land the pull request.",
		validation.issue.identifier,
		landing_url(&validation.landing_state)
	);

	tracker::create_linear_execution_event_comment(
		&context.tracker,
		&validation.issue.id,
		&body,
		event,
	)?;

	Ok(())
}

fn landing_url(landing_state: &PullRequestLandingState) -> &str {
	&landing_state.url
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

fn worktree_checkout_branch_name(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(trimmed_stdout(&output.stdout)?));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect retained worktree branch in `{}`: {}",
		worktree_path.display(),
		stderr.trim()
	)
}

fn worktree_head_oid(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["rev-parse", "--verify", "HEAD"])
		.output()?;

	if output.status.success() {
		return Ok(Some(trimmed_stdout(&output.stdout)?));
	}
	if output.status.code() == Some(128) {
		return Ok(None);
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect retained worktree HEAD in `{}`: {}",
		worktree_path.display(),
		stderr.trim()
	)
}

fn worktree_is_clean(worktree_path: &Path) -> Result<bool> {
	Ok(worktree_blocking_status_lines(worktree_path)?.is_empty())
}

fn worktree_blocking_status_lines(worktree_path: &Path) -> Result<Vec<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["status", "--porcelain"])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect retained worktree cleanliness in `{}`: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let status = String::from_utf8(output.stdout)?;

	Ok(status
		.lines()
		.filter(|line| !line.trim_end().is_empty())
		.filter(|line| !is_untracked_runtime_marker(line))
		.map(ToOwned::to_owned)
		.collect())
}

fn is_untracked_runtime_marker(line: &str) -> bool {
	line.trim_end().strip_prefix("?? ") == Some(RUN_ACTIVITY_MARKER_FILE)
}

fn trimmed_stdout(stdout: &[u8]) -> Result<String> {
	Ok(String::from_utf8(stdout.to_vec())?.trim().to_owned())
}

fn repository_relative_path(repo_root: &Path, path: &Path) -> Option<String> {
	let canonical_repo_root = fs::canonicalize(repo_root).ok()?;
	let canonical_path = fs::canonicalize(path).ok()?;
	let relative = canonical_path.strip_prefix(canonical_repo_root).ok()?;

	Some(relative.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
	use crate::{
		recovery::REVIEW_HANDOFF_REBIND_EVENT,
		tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	};

	#[test]
	fn review_handoff_rebind_event_validation_accepts_required_fields() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "issue-id",
				issue_identifier: "PUB-718",
				run_id: "pub-718-attempt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_REBIND_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("x/pubfi-pub-718"));
		record.worktree_path = Some(String::from(".worktrees/PUB-718"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary = Some(String::from("Explicit operator rebind restored marker."));
		record.evidence = Some(vec![String::from("existing_review_handoff_marker=absent")]);

		records::validate_linear_execution_event_record(&record)
			.expect("rebind event should validate");
	}

	#[test]
	fn review_handoff_rebind_event_requires_evidence() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "issue-id",
				issue_identifier: "PUB-718",
				run_id: "pub-718-attempt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_REBIND_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("x/pubfi-pub-718"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/pubfi-mono-v2/pull/14"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary = Some(String::from("Explicit operator rebind restored marker."));

		let error = records::validate_linear_execution_event_record(&record)
			.expect_err("rebind event without evidence should fail");

		assert!(error.contains("evidence"));
	}
}
