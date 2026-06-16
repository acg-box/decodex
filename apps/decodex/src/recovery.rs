//! Explicit operator recovery surfaces for retained Decodex lanes.

use std::{
	collections::HashMap,
	env, fs,
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::{Report, eyre::WrapErr};
use serde::Serialize;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	github,
	prelude::{Result, eyre},
	pull_request::{self, PullRequestLandingState},
	runtime,
	state::{
		self, ConnectorBackoffInput, ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore,
		WorktreeMapping,
	},
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::{WorkflowDocument, WorkflowTracker},
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_BOUND_CLASSIFICATION: &str = "review_handoff_bound";
const REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION: &str = "review_handoff_ownership_drift";
const REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION: &str = "review_handoff_rebind_required";
const REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION: &str = "review_handoff_unverified";
const REVIEW_HANDOFF_MISMATCH_CLASSIFICATION: &str = "review_handoff_mismatch";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
const REVIEW_HANDOFF_ADOPT_EVENT: &str = "review_handoff_adopt";
const LEGACY_MANUAL_CLOSEOUT_EVENT: &str = "closeout";
const LEGACY_MANUAL_CLOSEOUT_ANCHOR: &str = "legacy_manual_closeout";
const MERGED_CLOSEOUT_CLOSEOUT_ANCHOR: &str = "merged_closeout";
const MERGED_CLOSEOUT_CLEANUP_ANCHOR: &str = "merged_closeout_cleanup";
const REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";
const LINEAR_CONNECTOR_BACKOFF_WARNING: &str = "tracker_rate_limited";
const LINEAR_CONNECTOR_BACKOFF_SECS: i64 = 15 * 60;

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

/// Explicit manual PR takeover into retained review handoff state.
#[derive(Debug)]
pub(crate) struct ReviewHandoffAdoptRequest {
	/// Issue identifier to adopt.
	pub(crate) issue: String,
	/// Pull request URL to adopt.
	pub(crate) pr_url: String,
	/// Validate without writing runtime markers or tracker audit comments.
	pub(crate) dry_run: bool,
}

/// Explicit legacy closeout audit request.
#[derive(Debug)]
pub(crate) struct LegacyCloseoutRecoveryRequest {
	/// Issue identifier to audit.
	pub(crate) issue: String,
	/// Merged pull request URL that proves terminal code lineage.
	pub(crate) pr_url: String,
	/// Validate without writing a tracker audit comment.
	pub(crate) dry_run: bool,
	/// Required for non-dry-run mutation.
	pub(crate) manual_authority: bool,
}

/// Explicit merged PR closeout reconciliation for stale retained attention.
#[derive(Debug)]
pub(crate) struct MergedCloseoutRecoveryRequest {
	/// Issue identifier to reconcile.
	pub(crate) issue: String,
	/// Merged pull request URL that proves terminal code lineage.
	pub(crate) pr_url: String,
	/// Validate without writing runtime or tracker ledger events.
	pub(crate) dry_run: bool,
	/// Required for non-dry-run mutation.
	pub(crate) manual_authority: bool,
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
	existing_marker_head_oid: Option<String>,
	existing_orchestration_head_oid: Option<String>,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	mismatched_field: Option<String>,
	active_label_present: Option<bool>,
	next_action: String,
}

struct HandoffBindingDiagnostic {
	classification: String,
	reason: String,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	mismatched_field: Option<String>,
	next_action: String,
}

struct HandoffDiagnosticRequest<'a> {
	service_id: &'a str,
	issue_identifier: &'a str,
	issue_state_name: &'a str,
	success_state: &'a str,
	in_progress_state: &'a str,
	worktree: &'a WorktreeMapping,
	existing_handoff: Option<&'a ReviewHandoffMarker>,
	existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	local_branch_name: Option<&'a str>,
	local_head_oid: Option<&'a str>,
	worktree_clean: Option<bool>,
	pr_inspection: Option<&'a PullRequestLandingState>,
	active_label_present: Option<bool>,
}

struct HandoffDiagnosticContext<'a> {
	issue_identifier: &'a str,
	worktree: &'a WorktreeMapping,
	existing_handoff: &'a ReviewHandoffMarker,
	existing_orchestration: Option<&'a ReviewOrchestrationMarker>,
	local_branch_name: Option<&'a str>,
	local_head_oid: Option<&'a str>,
	worktree_clean: Option<bool>,
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
	run_id: String,
	attempt_number: i64,
	landing_state: PullRequestLandingState,
	local_head_oid: String,
	worktree_path_for_event: Option<String>,
	active_label_present: bool,
	mode: RebindMode,
	success_state_transition: Option<RebindSuccessStateTransition>,
	clear_needs_attention_label: bool,
}

struct AdoptValidation {
	issue: TrackerIssue,
	branch_name: String,
	worktree_path: PathBuf,
	run_id: String,
	attempt_number: i64,
	landing_state: PullRequestLandingState,
	local_head_oid: String,
	worktree_path_for_event: Option<String>,
	active_label_present: bool,
	success_state_transition: Option<RebindSuccessStateTransition>,
	previous_worktree_mapping: Option<WorktreeMapping>,
}
impl AdoptValidation {
	fn should_restore_active_label(&self) -> bool {
		!self.active_label_present
	}
}

struct LegacyCloseoutValidation {
	issue: TrackerIssue,
	worktree: WorktreeMapping,
	landing_state: PullRequestLandingState,
	local_head_oid: String,
	merge_commit: String,
	worktree_path_for_event: Option<String>,
}

struct MergedCloseoutValidation {
	issue: TrackerIssue,
	branch_name: String,
	worktree_path_for_event: String,
	run_id: String,
	attempt_number: i64,
	landing_state: PullRequestLandingState,
	merge_commit: String,
	worktree_mapping: Option<WorktreeMapping>,
}

struct MergedCloseoutRetainedContext {
	branch_name: String,
	worktree_path: String,
	run_id: String,
	attempt_number: i64,
}

#[derive(Debug)]
struct RebindSuccessStateTransition {
	state_name: String,
	state_id: String,
}

struct RebindLabelValidation {
	active_label_present: bool,
	clear_needs_attention_label: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RebindMode {
	RestoreMissingHandoff,
	RefreshExistingHandoff,
	CompleteExistingHandoffState,
}
impl RebindMode {
	fn allows_failure_state_drift_repair(self) -> bool {
		self == Self::CompleteExistingHandoffState
	}

	fn allows_partial_handoff_state_completion(self) -> bool {
		matches!(self, Self::RestoreMissingHandoff | Self::CompleteExistingHandoffState)
	}

	fn evidence_value(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "absent",
			Self::RefreshExistingHandoff => "refreshed",
			Self::CompleteExistingHandoffState => "current_state_transition",
		}
	}

	fn summary_action(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "restored retained review handoff marker",
			Self::RefreshExistingHandoff => "refreshed retained review handoff marker",
			Self::CompleteExistingHandoffState => "completed retained review handoff state",
		}
	}
}

enum ReviewHandoffLineage {
	Descends,
	Diverged,
	Unknown,
}

/// Run a read-only retained review handoff diagnostic.
pub(crate) fn run_review_handoff_diagnose(
	config_path: Option<&Path>,
	request: &ReviewHandoffDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let diagnostics = match match request.issue.as_deref() {
		Some(issue_identifier) =>
			diagnose_issue(&context, issue_identifier).map(|diagnostic| vec![diagnostic]),
		None => diagnose_all_retained_review_worktrees(&context),
	} {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) = remember_recovery_tracker_backoff_message(
				&context,
				&error,
				"review_handoff_recovery",
			) {
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
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
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());

		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} mode={} active_label_present={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.mode.evidence_value(),
			validation.active_label_present,
			state_transition
		);

		return Ok(());
	}

	apply_review_handoff_rebind(&context, &validation)?;

	println!(
		"rebind ok: project={} issue={} branch={} pr={} head={} mode={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.mode.evidence_value()
	);

	Ok(())
}

/// Run an explicit manual PR takeover into retained review handoff state.
pub(crate) fn run_review_handoff_adopt(
	config_path: Option<&Path>,
	request: &ReviewHandoffAdoptRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;
	let validation = validate_adopt_request(&context, request)?;

	if request.dry_run {
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());
		let active_label = tracker::automation_active_label(context.config.service_id());

		println!(
			"dry run: review handoff adopt validated for project={} issue={} branch={} pr={} head={} run_id={} attempt={} active_label={} active_label_present={} would_restore_active_label={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.branch_name,
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.run_id,
			validation.attempt_number,
			active_label,
			validation.active_label_present,
			validation.should_restore_active_label(),
			state_transition
		);

		return Ok(());
	}

	apply_review_handoff_adopt(&context, &validation)?;

	println!(
		"adopt ok: project={} issue={} branch={} pr={} head={} run_id={} attempt={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.run_id,
		validation.attempt_number
	);

	Ok(())
}

/// Run an explicit audited legacy closeout fallback.
pub(crate) fn run_legacy_closeout(
	config_path: Option<&Path>,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;
	let validation = validate_legacy_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: legacy closeout validated for project={} issue={} branch={} pr={} head={} merge_commit={} provenance={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.merge_commit,
			validation.worktree.provenance().source()
		);

		return Ok(());
	}
	if !request.manual_authority {
		eyre::bail!(
			"`recover legacy-closeout` writes a closeout audit and requires --manual-authority outside dry-run mode."
		);
	}

	let event = legacy_closeout_event(&context, &validation);
	let audit_recorded = write_legacy_closeout_audit(&context, &validation, &event)?;

	println!(
		"legacy closeout audit ok: project={} issue={} branch={} pr={} head={} merge_commit={} audit_recorded={audit_recorded}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.local_head_oid,
		validation.merge_commit,
	);

	Ok(())
}

/// Run an explicit merged PR closeout reconciliation for stale retained attention.
pub(crate) fn run_merged_closeout(
	config_path: Option<&Path>,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<()> {
	let context = load_recovery_context(config_path)?;
	let validation = validate_merged_closeout_request(&context, request)?;

	if request.dry_run {
		println!(
			"dry run: merged closeout validated for project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} run_id={} attempt={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.branch_name,
			validation.worktree_path_for_event,
			landing_url(&validation.landing_state),
			validation.landing_state.head_ref_oid,
			validation.merge_commit,
			validation.run_id,
			validation.attempt_number
		);

		return Ok(());
	}
	if !request.manual_authority {
		eyre::bail!(
			"`recover merged-closeout` writes closeout and cleanup ledger records and requires --manual-authority outside dry-run mode."
		);
	}

	let (closeout_recorded, cleanup_recorded) =
		apply_merged_closeout_recovery(&context, &validation)?;

	println!(
		"merged closeout recovery ok: project={} issue={} branch={} worktree_path={} pr={} head={} merge_commit={} closeout_recorded={} cleanup_recorded={}",
		context.config.service_id(),
		validation.issue.identifier,
		validation.branch_name,
		validation.worktree_path_for_event,
		landing_url(&validation.landing_state),
		validation.landing_state.head_ref_oid,
		validation.merge_commit,
		closeout_recorded,
		cleanup_recorded
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

fn active_recovery_tracker_backoff_message(context: &RecoveryContext) -> Result<Option<String>> {
	let Some(backoff) =
		context.state_store.connector_backoff(context.config.service_id(), "linear")?
	else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		context.state_store.clear_connector_backoff(context.config.service_id(), "linear")?;

		return Ok(None);
	}

	Ok(Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		backoff.sync_phase(),
		backoff.reset_unix_epoch(),
		backoff.reset_unix_epoch().saturating_sub(now_unix_epoch),
	)))
}

fn remember_recovery_tracker_backoff_message(
	context: &RecoveryContext,
	error: &Report,
	sync_phase: &str,
) -> Option<String> {
	let message = format!("{error:#}");

	if !message.contains("Linear connector is rate limited") {
		return None;
	}

	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (reset_unix_epoch, reset_source) =
		match parse_recovery_rate_limit_reset_unix_epoch(&message) {
			Some(reset) if reset > now_unix_epoch => (reset, "linear"),
			_ => (now_unix_epoch.saturating_add(LINEAR_CONNECTOR_BACKOFF_SECS), "local_default"),
		};

	if let Err(store_error) = context.state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id: context.config.service_id(),
		connector: "linear",
		sync_phase,
		quota_class: "linear_graphql_api",
		reset_unix_epoch,
		reset_source,
		warning: LINEAR_CONNECTOR_BACKOFF_WARNING,
	}) {
		let _ = store_error;

		tracing::warn!(
			project_id = context.config.service_id(),
			"Failed to persist recovery tracker backoff; sensitive runtime details were withheld."
		);
	}

	Some(recovery_tracker_backoff_message(
		context.config.service_id(),
		sync_phase,
		reset_unix_epoch,
		reset_unix_epoch.saturating_sub(now_unix_epoch),
	))
}

fn parse_recovery_rate_limit_reset_unix_epoch(message: &str) -> Option<i64> {
	let reset = message.split("rate limited until `").nth(1)?.split('`').next()?;

	reset.parse().ok()
}

fn recovery_tracker_backoff_message(
	service_id: &str,
	sync_phase: &str,
	reset_unix_epoch: i64,
	retry_after_seconds: i64,
) -> String {
	format!(
		"Linear connector is in backoff for project `{service_id}`; recovery skipped tracker reads for `{sync_phase}` until unix_epoch={reset_unix_epoch} (retry_after_seconds={retry_after_seconds})."
	)
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
			"No Decodex project config found. Pass this command's --config <PROJECT_DIR> or register one with `decodex project add <PROJECT_DIR>`."
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
	let tracker_policy = context.workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let in_progress_state = tracker_policy.in_progress_state();
	let mut diagnostics = Vec::new();

	for worktree in worktrees {
		let Some(issue) = issues_by_id.get(worktree.issue_id()).cloned() else {
			continue;
		};

		if issue.state.name != success_state && issue.state.name != in_progress_state {
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
	let existing_orchestration = existing_handoff
		.as_ref()
		.map(|handoff| {
			context.state_store.review_orchestration_marker(
				context.config.service_id(),
				&issue.id,
				handoff,
			)
		})
		.transpose()?
		.flatten();
	let pr_inspection = existing_handoff
		.as_ref()
		.and_then(|handoff| inspect_project_pull_request(context, handoff.pr_url()).ok())
		.map(|(landing_state, _default_branch)| landing_state);
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
	let binding = diagnostic_binding(HandoffDiagnosticRequest {
		service_id: context.config.service_id(),
		issue_identifier: &issue.identifier,
		issue_state_name: &issue.state.name,
		success_state: context.workflow.frontmatter().tracker().success_state(),
		in_progress_state: context.workflow.frontmatter().tracker().in_progress_state(),
		worktree: &worktree,
		existing_handoff: existing_handoff.as_ref(),
		existing_orchestration: existing_orchestration.as_ref(),
		local_branch_name: local_branch_name.as_deref(),
		local_head_oid: local_head_oid.as_deref(),
		worktree_clean,
		pr_inspection: pr_inspection.as_ref(),
		active_label_present,
	});

	Ok(ReviewHandoffDiagnostic {
		project_id: context.config.service_id().to_owned(),
		issue_id: issue.id.clone(),
		issue_identifier: issue.identifier.clone(),
		issue_state: issue.state.name.clone(),
		classification: binding.classification,
		reason: binding.reason,
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: worktree.worktree_path().display().to_string(),
		local_branch_name,
		local_head_oid,
		worktree_clean,
		existing_pr_url: existing_handoff.as_ref().map(|handoff| handoff.pr_url().to_owned()),
		existing_marker_head_oid: existing_handoff
			.as_ref()
			.map(|handoff| handoff.pr_head_oid().to_owned()),
		existing_orchestration_head_oid: existing_orchestration
			.as_ref()
			.map(|orchestration| orchestration.head_sha().to_owned()),
		pr_base_ref: binding.pr_base_ref,
		pr_head_oid: binding.pr_head_oid,
		mismatched_field: binding.mismatched_field,
		active_label_present,
		next_action: binding.next_action,
	})
}

fn diagnostic_binding(request: HandoffDiagnosticRequest<'_>) -> HandoffBindingDiagnostic {
	let Some(existing_handoff) = request.existing_handoff else {
		return HandoffBindingDiagnostic {
			classification: String::from(ORPHANED_REVIEW_HANDOFF_CLASSIFICATION),
			reason: String::from(MISSING_HANDOFF_REASON),
			pr_base_ref: None,
			pr_head_oid: None,
			mismatched_field: None,
			next_action: missing_handoff_next_action(request.service_id, request.issue_identifier),
		};
	};
	let context = HandoffDiagnosticContext {
		issue_identifier: request.issue_identifier,
		worktree: request.worktree,
		existing_handoff,
		existing_orchestration: request.existing_orchestration,
		local_branch_name: request.local_branch_name,
		local_head_oid: request.local_head_oid,
		worktree_clean: request.worktree_clean,
	};
	let pr_base_ref = request.pr_inspection.map(|pr| pr.base_ref_name.clone());
	let pr_head_oid = request.pr_inspection.map(|pr| pr.head_ref_oid.clone());

	if let Some(diagnostic) = worktree_binding_mismatch(&context, &pr_base_ref, &pr_head_oid) {
		return diagnostic;
	}

	let Some(local_head_oid) = request.local_head_oid else {
		return mismatched_handoff_diagnostic(
			"worktree_head_missing",
			"worktree.local_head",
			pr_base_ref,
			pr_head_oid,
			inspect_handoff_next_action(request.issue_identifier, existing_handoff.pr_url()),
		);
	};
	let Some(pr_inspection) = request.pr_inspection else {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION),
			reason: String::from("pull_request_state_read_failed"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("pr_url")),
			next_action: inspect_handoff_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		};
	};

	if let Some(diagnostic) =
		pull_request_binding_mismatch(&context, pr_inspection, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}
	if let Some(diagnostic) =
		marker_head_binding_mismatch(&context, local_head_oid, &pr_base_ref, &pr_head_oid)
	{
		return diagnostic;
	}

	if request.active_label_present == Some(false) {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION),
			reason: String::from("active_ownership_label_missing"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.labels")),
			next_action: bound_handoff_next_action(
				request.service_id,
				request.active_label_present,
			),
		};
	}
	if request.issue_state_name == request.in_progress_state {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_state_transition_pending"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		};
	}
	if request.issue_state_name != request.success_state {
		return HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
			reason: String::from("review_handoff_issue_state_mismatch"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: issue_state_mismatch_next_action(
				request.success_state,
				request.in_progress_state,
			),
		};
	}

	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_BOUND_CLASSIFICATION),
		reason: String::from("review_handoff_record_present"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: None,
		next_action: bound_handoff_next_action(request.service_id, request.active_label_present),
	}
}

fn worktree_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.branch_name() != context.worktree.branch_name() {
		Some(("review_handoff_branch_mismatch", "review_handoff.branch_name"))
	} else if context.local_branch_name.is_none() {
		Some(("worktree_checkout_branch_missing", "worktree.local_branch"))
	} else if context.local_branch_name != Some(context.worktree.branch_name()) {
		Some(("worktree_checkout_branch_mismatch", "worktree.local_branch"))
	} else if context.worktree_clean == Some(false) {
		Some(("worktree_dirty", "worktree.clean"))
	} else if context.local_head_oid.is_none() {
		Some(("worktree_head_missing", "worktree.local_head"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn pull_request_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	pr_inspection: &PullRequestLandingState,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if context
		.existing_handoff
		.target_base_ref_name()
		.is_some_and(|base_ref| base_ref != pr_inspection.base_ref_name.as_str())
	{
		return Some(rebind_required_diagnostic(
			"review_handoff_base_mismatch",
			"review_handoff.target_base_ref_name",
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		));
	}

	let mismatch = if pr_inspection.head_ref_name != context.worktree.branch_name() {
		Some(("pull_request_branch_mismatch", "pull_request.head_ref_name"))
	} else if context.local_head_oid != Some(pr_inspection.head_ref_oid.as_str()) {
		Some(("pull_request_head_mismatch", "pull_request.head_ref_oid"))
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		mismatched_handoff_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			inspect_handoff_next_action(
				context.issue_identifier,
				context.existing_handoff.pr_url(),
			),
		)
	})
}

fn marker_head_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	local_head_oid: &str,
	pr_base_ref: &Option<String>,
	pr_head_oid: &Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	let mismatch = if context.existing_handoff.pr_head_oid() != local_head_oid {
		match worktree_head_descends_from_review_handoff(
			context.worktree.worktree_path(),
			context.existing_handoff.pr_head_oid(),
			local_head_oid,
		) {
			ReviewHandoffLineage::Descends => None,
			ReviewHandoffLineage::Diverged =>
				Some(("review_handoff_lineage_mismatch", "review_handoff.pr_head_oid")),
			ReviewHandoffLineage::Unknown =>
				Some(("review_handoff_lineage_check_failed", "review_handoff.pr_head_oid")),
		}
	} else if let Some(orchestration) = context.existing_orchestration {
		orchestration_binding_mismatch(context, orchestration, local_head_oid)
	} else {
		None
	};

	mismatch.map(|(reason, field)| {
		rebind_required_diagnostic(
			reason,
			field,
			pr_base_ref.clone(),
			pr_head_oid.clone(),
			context.issue_identifier,
			context.existing_handoff.pr_url(),
		)
	})
}

fn orchestration_binding_mismatch(
	context: &HandoffDiagnosticContext<'_>,
	orchestration: &ReviewOrchestrationMarker,
	local_head_oid: &str,
) -> Option<(&'static str, &'static str)> {
	if orchestration.branch_name() != context.worktree.branch_name() {
		Some(("review_orchestration_branch_mismatch", "review_orchestration.branch_name"))
	} else if orchestration.pr_url() != context.existing_handoff.pr_url() {
		Some(("review_orchestration_pr_mismatch", "review_orchestration.pr_url"))
	} else if orchestration.head_sha() != local_head_oid {
		Some(("review_orchestration_head_mismatch", "review_orchestration.head_sha"))
	} else {
		None
	}
}

fn mismatched_handoff_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	next_action: String,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action,
	}
}

fn rebind_required_diagnostic(
	reason: &str,
	mismatched_field: &str,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	issue_identifier: &str,
	pr_url: &str,
) -> HandoffBindingDiagnostic {
	HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
		reason: reason.to_owned(),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(mismatched_field.to_owned()),
		next_action: rebind_refresh_next_action(issue_identifier, pr_url),
	}
}

fn missing_handoff_next_action(service_id: &str, issue_identifier: &str) -> String {
	format!(
		"Inspect PR lineage and ensure label `{}` is present. Use `decodex recover review-handoff rebind {} --pr <URL>` for a retained lane PR, or `decodex recover review-handoff adopt {} --pr <URL>` from the managed worktree for a human-owned PR takeover.",
		tracker::automation_active_label(service_id),
		issue_identifier,
		issue_identifier
	)
}

fn bound_handoff_next_action(service_id: &str, active_label_present: Option<bool>) -> String {
	if active_label_present == Some(false) {
		return format!(
			"Restore explicit lane ownership with label `{}`, then rerun `decodex recover review-handoff diagnose <ISSUE>` and continue the existing post-review lifecycle.",
			tracker::automation_active_label(service_id)
		);
	}

	String::from("Continue the existing post-review lifecycle; no rebind is needed.")
}

fn inspect_handoff_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Inspect the retained worktree and PR `{pr_url}`; run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url}` only after the mismatch is repaired."
	)
}

fn rebind_refresh_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to refresh the retained marker if validation passes."
	)
}

fn rebind_state_transition_next_action(issue_identifier: &str, pr_url: &str) -> String {
	format!(
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to complete the pending issue-state transition if validation passes."
	)
}

fn issue_state_mismatch_next_action(success_state: &str, in_progress_state: &str) -> String {
	format!(
		"Move the issue to `{success_state}` or `{in_progress_state}` only after confirming the retained handoff lineage still belongs to the current lane."
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
			"- issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  local_branch: {}\n  local_head: {}\n  worktree_clean: {}\n  existing_pr_url: {}\n  existing_marker_head: {}\n  existing_orchestration_head: {}\n  pr_base_ref: {}\n  pr_head: {}\n  mismatched_field: {}\n  active_label_present: {}\n  next_action: {}\n",
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
			optional_text(diagnostic.existing_marker_head_oid.as_deref()),
			optional_text(diagnostic.existing_orchestration_head_oid.as_deref()),
			optional_text(diagnostic.pr_base_ref.as_deref()),
			optional_text(diagnostic.pr_head_oid.as_deref()),
			optional_text(diagnostic.mismatched_field.as_deref()),
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
	let existing_handoff = context.state_store.review_handoff_marker(
		context.config.service_id(),
		&issue.id,
		worktree.branch_name(),
	)?;
	let landing_state = inspect_rebind_pull_request(context, &request.pr_url)?;
	let local_head_oid = validate_rebind_worktree(&worktree, &landing_state)?;
	let existing_orchestration = existing_handoff
		.as_ref()
		.map(|handoff| {
			context.state_store.review_orchestration_marker(
				context.config.service_id(),
				&issue.id,
				handoff,
			)
		})
		.transpose()?
		.flatten();
	let (run_id, attempt_number, mode) = validate_rebind_existing_handoff(
		context,
		&issue,
		&worktree,
		existing_handoff.as_ref(),
		existing_orchestration.as_ref(),
		&landing_state,
		&local_head_oid,
	)?;
	let success_state_transition = validate_rebind_issue_state(context, &issue, mode)?;
	let label_validation = validate_rebind_tracker_labels(context, &issue, mode)?;
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), worktree.worktree_path());

	Ok(RebindValidation {
		issue,
		worktree,
		run_id,
		attempt_number,
		landing_state,
		local_head_oid,
		worktree_path_for_event,
		active_label_present: label_validation.active_label_present,
		mode,
		success_state_transition,
		clear_needs_attention_label: label_validation.clear_needs_attention_label,
	})
}

fn validate_adopt_request(
	context: &RecoveryContext,
	request: &ReviewHandoffAdoptRequest,
) -> Result<AdoptValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;
	let label_validation = validate_adopt_issue_context(context, &issue)?;
	let landing_state = inspect_rebind_pull_request(context, &request.pr_url)?;
	let existing_worktree_mapping = context.state_store.worktree_for_issue(&issue.id)?;

	validate_adopt_landing_state(&landing_state)?;

	let cwd = env::current_dir()?;
	let worktree_path = validate_adopt_current_worktree(
		context,
		&issue,
		&landing_state,
		&cwd,
		existing_worktree_mapping.as_ref(),
	)?;
	let branch_name = worktree_checkout_branch_name(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;
	let local_head_oid = worktree_head_oid(&worktree_path)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	validate_adopt_absent_handoff_marker(
		context,
		&issue,
		&branch_name,
		existing_worktree_mapping.as_ref(),
	)?;

	let success_state_transition = validate_adopt_issue_state(context, &issue)?;
	let attempt_number = context
		.state_store
		.latest_run_attempt_for_issue(&issue.id)?
		.map_or(1, |attempt| attempt.attempt_number().saturating_add(1));
	let run_id = manual_adopt_run_id(&issue.identifier, attempt_number, &local_head_oid);
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), &worktree_path);

	Ok(AdoptValidation {
		issue,
		branch_name,
		worktree_path,
		run_id,
		attempt_number,
		landing_state,
		local_head_oid,
		worktree_path_for_event,
		active_label_present: label_validation.active_label_present,
		success_state_transition,
		previous_worktree_mapping: existing_worktree_mapping,
	})
}

fn validate_legacy_closeout_request(
	context: &RecoveryContext,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<LegacyCloseoutValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_legacy_closeout_issue_state(context.workflow.frontmatter().tracker(), &issue)?;

	let worktree = legacy_closeout_worktree(context, &issue)?;

	if !worktree.provenance().is_legacy_unknown() {
		eyre::bail!(
			"Issue `{}` worktree provenance is `{}`; legacy closeout requires `legacy_unknown` cleanup-only provenance.",
			issue.identifier,
			worktree.provenance().source()
		);
	}

	let (landing_state, default_branch) = inspect_project_pull_request(context, &request.pr_url)?;

	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			request.pr_url,
			landing_state.base_ref_name,
			default_branch
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; legacy closeout requires `MERGED`.",
			request.pr_url,
			landing_state.state
		);
	}

	let local_head_oid = validate_legacy_closeout_worktree(&worktree, &landing_state)?;
	let merge_commit = inspect_project_pull_request_merge_commit(context, &request.pr_url)?;
	let worktree_path_for_event =
		repository_relative_path(context.config.repo_root(), worktree.worktree_path());

	Ok(LegacyCloseoutValidation {
		issue,
		worktree,
		landing_state,
		local_head_oid,
		merge_commit,
		worktree_path_for_event,
	})
}

fn validate_merged_closeout_request(
	context: &RecoveryContext,
	request: &MergedCloseoutRecoveryRequest,
) -> Result<MergedCloseoutValidation> {
	let issue = load_issue_by_identifier(&context.tracker, &request.issue)?;

	validate_merged_closeout_issue_context(context, &issue)?;

	let (landing_state, default_branch) = inspect_project_pull_request(context, &request.pr_url)?;

	validate_merged_closeout_pull_request(context, &landing_state, &default_branch)?;

	let merge_commit = inspect_project_pull_request_merge_commit(context, &request.pr_url)?;

	ensure_merge_commit_reachable_from_remote_default_branch(
		context.config.repo_root(),
		&request.pr_url,
		&merge_commit,
		&default_branch,
	)?;

	let worktree_mapping = retained_worktree_mapping_for_issue(context, &issue)?;
	let retained_context =
		merged_closeout_retained_context(context, &issue, worktree_mapping.as_ref())?;

	if landing_state.head_ref_name != retained_context.branch_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but retained lane branch is `{}`.",
			landing_url(&landing_state),
			landing_state.head_ref_name,
			retained_context.branch_name
		);
	}

	validate_merged_closeout_worktree_mapping(
		context,
		&issue,
		worktree_mapping.as_ref(),
		&landing_state,
	)?;

	Ok(MergedCloseoutValidation {
		issue,
		branch_name: retained_context.branch_name,
		worktree_path_for_event: retained_context.worktree_path,
		run_id: retained_context.run_id,
		attempt_number: retained_context.attempt_number,
		landing_state,
		merge_commit,
		worktree_mapping,
	})
}

fn validate_legacy_closeout_issue_state(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<()> {
	if tracker_policy.terminal_states().iter().any(|state| state == &issue.state.name) {
		return Ok(());
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but legacy closeout requires a terminal state: {}.",
		issue.identifier,
		issue.state.name,
		tracker_policy.terminal_states().join(", ")
	)
}

fn legacy_closeout_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<WorktreeMapping> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(worktree);
	}
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.identifier)? {
		return Ok(worktree);
	}

	eyre::bail!("Issue `{}` has no retained worktree mapping.", issue.identifier)
}

fn validate_merged_closeout_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<()> {
	let tracker_policy = context.workflow.frontmatter().tracker();
	let completed_state = tracker_policy.resolved_completed_state();

	if issue.state.name != completed_state {
		eyre::bail!(
			"Issue `{}` is in `{}`, but merged closeout recovery requires `{completed_state}`.",
			issue.identifier,
			issue.state.name
		);
	}
	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	for label in [
		tracker::automation_queue_label(context.config.service_id()),
		tracker::automation_active_label(context.config.service_id()),
		tracker_policy.needs_attention_label().to_owned(),
	] {
		if tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &label)? {
			eyre::bail!(
				"Issue `{}` still has Linear label `{label}`; merged closeout recovery requires queue, active, and needs-attention labels to be absent.",
				issue.identifier
			);
		}
	}

	Ok(())
}

fn retained_worktree_mapping_for_issue(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<WorktreeMapping>> {
	if let Some(worktree) = context.state_store.worktree_for_issue(&issue.id)? {
		return Ok(Some(worktree));
	}

	context.state_store.worktree_for_issue(&issue.identifier)
}

fn merged_closeout_retained_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
) -> Result<MergedCloseoutRetainedContext> {
	let latest_record = latest_merged_closeout_source_record(context, issue)?;
	let branch_name = worktree_mapping
		.map(|mapping| mapping.branch_name().to_owned())
		.or_else(|| latest_record.as_ref().and_then(|record| record.branch.clone()))
		.ok_or_else(|| {
			eyre::eyre!(
				"Issue `{}` has no retained branch in runtime state or execution ledger.",
				issue.identifier
			)
		})?;
	let worktree_path = worktree_mapping
		.and_then(|mapping| relative_worktree_path_for_recovery(context, mapping.worktree_path()))
		.or_else(|| latest_record.as_ref().and_then(|record| record.worktree_path.clone()))
		.unwrap_or_else(|| format!(".worktrees/{}", issue.identifier));
	let (run_id, attempt_number) = if let Some(record) = latest_record
		.as_ref()
		.filter(|record| !record.run_id.trim().is_empty() && record.attempt_number >= 1)
	{
		(record.run_id.clone(), record.attempt_number)
	} else if let Some(attempt) = context.state_store.latest_run_attempt_for_issue(&issue.id)? {
		(attempt.run_id().to_owned(), attempt.attempt_number())
	} else {
		(format!("merged-closeout-{}", issue.identifier.to_ascii_lowercase()), 1)
	};

	Ok(MergedCloseoutRetainedContext { branch_name, worktree_path, run_id, attempt_number })
}

fn latest_merged_closeout_source_record(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<LinearExecutionEventRecord>> {
	let mut records =
		context.state_store.list_linear_execution_events(context.config.service_id(), &issue.id)?;

	if issue.identifier != issue.id {
		records.extend(
			context
				.state_store
				.list_linear_execution_events(context.config.service_id(), &issue.identifier)?,
		);
	}

	let comments = context.tracker.list_comments(&issue.id)?;

	records.extend(
		comments
			.iter()
			.filter_map(|comment| records::parse_linear_execution_event_record(&comment.body))
			.filter(|record| {
				record.service_id == context.config.service_id()
					&& (record.issue_id == issue.id || record.issue_identifier == issue.identifier)
			}),
	);

	Ok(records
		.into_iter()
		.filter(|record| record.branch.as_ref().is_some_and(|branch| !branch.trim().is_empty()))
		.max_by(|left, right| {
			left.event_timestamp
				.cmp(&right.event_timestamp)
				.then_with(|| left.idempotency_key.cmp(&right.idempotency_key))
		}))
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

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	Ok(worktree)
}

fn validate_rebind_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	validate_rebind_issue_state_for_policy(context.workflow.frontmatter().tracker(), issue, mode)
}

fn validate_rebind_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if mode.allows_partial_handoff_state_completion()
		&& issue.state.name == tracker_policy.in_progress_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but review handoff rebind requires `{}`{}.",
		issue.identifier,
		issue.state.name,
		success_state,
		if mode.allows_partial_handoff_state_completion() {
			format!(
				" or `{}`{} for a partial handoff recovery",
				tracker_policy.in_progress_state(),
				if mode.allows_failure_state_drift_repair() {
					format!(" or `{}` for state drift recovery", tracker_policy.failure_state())
				} else {
					String::new()
				}
			)
		} else {
			String::new()
		}
	)
}

fn validate_rebind_existing_handoff(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: Option<&ReviewHandoffMarker>,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	let Some(existing_handoff) = existing_handoff else {
		let attempt =
			context.state_store.latest_run_attempt_for_issue(&issue.id)?.ok_or_else(|| {
				eyre::eyre!("Issue `{}` has no recorded run attempt to rebind.", issue.identifier)
			})?;

		return Ok((
			attempt.run_id().to_owned(),
			attempt.attempt_number(),
			RebindMode::RestoreMissingHandoff,
		));
	};

	validate_existing_handoff_refresh(
		context.workflow.frontmatter().tracker(),
		issue,
		worktree,
		existing_handoff,
		existing_orchestration,
		landing_state,
		local_head_oid,
	)
}

fn validate_existing_handoff_refresh(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	worktree: &WorktreeMapping,
	existing_handoff: &ReviewHandoffMarker,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	if existing_handoff.pr_url() != landing_url(landing_state) {
		eyre::bail!(
			"Issue `{}` already has review handoff marker for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url(),
			landing_url(landing_state)
		);
	}

	let orchestration_is_current = existing_orchestration.is_none_or(|marker| {
		marker.branch_name() == worktree.branch_name()
			&& marker.pr_url() == landing_url(landing_state)
			&& marker.head_sha() == local_head_oid
	});

	if existing_handoff.pr_head_oid() == local_head_oid && orchestration_is_current {
		if issue.state.name == tracker_policy.in_progress_state()
			|| issue.state.name == tracker_policy.failure_state()
		{
			return Ok((
				existing_handoff.run_id().to_owned(),
				existing_handoff.attempt_number(),
				RebindMode::CompleteExistingHandoffState,
			));
		}

		eyre::bail!(
			"Issue `{}` already has a review handoff marker for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
			issue.identifier,
			worktree.branch_name(),
			existing_handoff.pr_url()
		);
	}

	Ok((
		existing_handoff.run_id().to_owned(),
		existing_handoff.attempt_number(),
		RebindMode::RefreshExistingHandoff,
	))
}

fn inspect_rebind_pull_request(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<PullRequestLandingState> {
	let (landing_state, default_branch) = inspect_project_pull_request(context, pr_url)?;

	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{}`.",
			pr_url,
			landing_state.base_ref_name,
			default_branch
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

fn inspect_project_pull_request(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<(PullRequestLandingState, String)> {
	let github_token = context.config.github().resolve_token()?;
	let repository = github::inspect_repository_context(
		context.config.repo_root(),
		&github_token,
		context.config.github().command_path(),
	)?;

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
		context.config.github().command_path(),
	)?;

	Ok((landing_state, repository.default_branch))
}

fn inspect_project_pull_request_merge_commit(
	context: &RecoveryContext,
	pr_url: &str,
) -> Result<String> {
	let github_token = context.config.github().resolve_token()?;

	github::inspect_pull_request_merge_commit(
		context.config.repo_root(),
		pr_url,
		&github_token,
		context.config.github().command_path(),
	)
}

fn validate_merged_closeout_pull_request(
	context: &RecoveryContext,
	landing_state: &PullRequestLandingState,
	default_branch: &str,
) -> Result<()> {
	if landing_state.base_ref_name != default_branch {
		eyre::bail!(
			"Pull request `{}` targets `{}`, but configured default branch is `{default_branch}`.",
			landing_url(landing_state),
			landing_state.base_ref_name
		);
	}
	if landing_state.state != "MERGED" {
		eyre::bail!(
			"Pull request `{}` is `{}`; merged closeout recovery requires `MERGED`.",
			landing_url(landing_state),
			landing_state.state
		);
	}
	if landing_state.head_ref_name.trim().is_empty() {
		eyre::bail!(
			"Pull request `{}` does not expose the merged head branch required for retained lane reconciliation.",
			landing_url(landing_state)
		);
	}
	if landing_state.head_ref_name == default_branch {
		eyre::bail!(
			"Pull request `{}` uses default branch `{default_branch}` as its head; merged closeout recovery cannot prove retained lane identity.",
			landing_url(landing_state)
		);
	}

	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let output = Command::new("git")
		.arg("-C")
		.arg(context.config.repo_root())
		.args(["rev-parse", "--verify", remote_ref.as_str()])
		.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Configured repo root `{}` does not expose `{remote_ref}`; sync the default branch before merged closeout recovery: {}",
			context.config.repo_root().display(),
			stderr.trim()
		);
	}

	Ok(())
}

fn ensure_merge_commit_reachable_from_remote_default_branch(
	repo_root: &Path,
	pr_url: &str,
	merge_commit: &str,
	default_branch: &str,
) -> Result<()> {
	let remote_ref = format!("refs/remotes/origin/{default_branch}");
	let status = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["merge-base", "--is-ancestor", merge_commit, remote_ref.as_str()])
		.status()?;

	if status.success() {
		return Ok(());
	}
	if status.code() == Some(1) {
		eyre::bail!(
			"Configured repo root `{}` remote `{remote_ref}` does not contain merge commit `{merge_commit}` for `{pr_url}`.",
			repo_root.display()
		);
	}

	eyre::bail!(
		"`git merge-base --is-ancestor {merge_commit} {remote_ref}` failed in `{}` with status `{status}`.",
		repo_root.display()
	)
}

fn validate_merged_closeout_worktree_mapping(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	worktree_mapping: Option<&WorktreeMapping>,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if let Some(mapping) = worktree_mapping {
		if mapping.branch_name() != landing_state.head_ref_name {
			eyre::bail!(
				"Issue `{}` retained worktree branch is `{}`, but merged PR head branch is `{}`.",
				issue.identifier,
				mapping.branch_name(),
				landing_state.head_ref_name
			);
		}

		return validate_merged_closeout_worktree_path(mapping.worktree_path(), landing_state);
	}

	let Some(relative_path) = latest_merged_closeout_source_record(context, issue)?
		.and_then(|record| record.worktree_path)
	else {
		return Ok(());
	};
	let worktree_path = context.config.repo_root().join(relative_path);

	validate_merged_closeout_worktree_path(&worktree_path, landing_state)
}

fn validate_merged_closeout_worktree_path(
	worktree_path: &Path,
	landing_state: &PullRequestLandingState,
) -> Result<()> {
	if !worktree_path.exists() {
		return Ok(());
	}
	if !worktree_is_clean(worktree_path)? {
		eyre::bail!(
			"Retained worktree `{}` still has local changes; merged closeout recovery will not mark it cleanup-complete.",
			worktree_path.display()
		);
	}

	let local_branch = worktree_checkout_branch_name(worktree_path)?.ok_or_else(|| {
		eyre::eyre!("Retained worktree `{}` is detached.", worktree_path.display())
	})?;

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Retained worktree `{}` is on branch `{local_branch}`, but merged PR head branch is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_name
		);
	}

	let local_head = worktree_head_oid(worktree_path)?.ok_or_else(|| {
		eyre::eyre!("Retained worktree `{}` has no readable HEAD.", worktree_path.display())
	})?;

	if local_head != landing_state.head_ref_oid {
		eyre::bail!(
			"Retained worktree `{}` HEAD is `{local_head}`, but merged PR head is `{}`.",
			worktree_path.display(),
			landing_state.head_ref_oid
		);
	}

	Ok(())
}

fn validate_rebind_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	validate_retained_pr_worktree(worktree, landing_state, "rebind")
}

fn validate_legacy_closeout_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
) -> Result<String> {
	validate_retained_pr_worktree(worktree, landing_state, "legacy closeout")
}

fn validate_retained_pr_worktree(
	worktree: &WorktreeMapping,
	landing_state: &PullRequestLandingState,
	action_label: &str,
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
			"Retained worktree `{}` has local changes; {action_label} requires a clean lane checkout.",
			worktree.worktree_path().display(),
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

fn validate_rebind_tracker_labels(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<RebindLabelValidation> {
	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;

	if !active_label_present {
		eyre::bail!(
			"Issue `{}` is missing active automation label `{active_label}`. Restore explicit lane ownership before rebind.",
			issue.identifier
		);
	}

	let tracker_policy = context.workflow.frontmatter().tracker();
	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if !needs_attention_present {
		return Ok(RebindLabelValidation {
			active_label_present,
			clear_needs_attention_label: false,
		});
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			needs_attention_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` has needs-attention label `{needs_attention_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}

		return Ok(RebindLabelValidation {
			active_label_present,
			clear_needs_attention_label: true,
		});
	}

	eyre::bail!(
		"Issue `{}` has needs-attention label `{}`.",
		issue.identifier,
		needs_attention_label
	)
}

fn validate_adopt_issue_context(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<RebindLabelValidation> {
	let tracker_policy = context.workflow.frontmatter().tracker();

	if issue.has_label(tracker_policy.opt_out_label()) {
		eyre::bail!(
			"Issue `{}` has opt-out label `{}`.",
			issue.identifier,
			tracker_policy.opt_out_label()
		);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(&context.tracker, issue, &active_label)?;

	if !active_label_present
		&& tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
	{
		eyre::bail!(
			"Issue `{}` is missing active automation label `{active_label}`, and that label was not found on the team.",
			issue.identifier
		);
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if needs_attention_present {
		eyre::bail!(
			"Issue `{}` has needs-attention label `{needs_attention_label}`; manual takeover adopt will not bypass a human-required stop.",
			issue.identifier
		);
	}

	Ok(RebindLabelValidation { active_label_present, clear_needs_attention_label: false })
}

fn validate_adopt_issue_state(
	context: &RecoveryContext,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	validate_adopt_issue_state_for_policy(context.workflow.frontmatter().tracker(), issue)
}

fn validate_adopt_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if issue.state.name == tracker_policy.in_progress_state() {
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but manual takeover adopt requires `{}` or `{}`.",
		issue.identifier,
		issue.state.name,
		tracker_policy.in_progress_state(),
		success_state
	)
}

fn validate_adopt_landing_state(landing_state: &PullRequestLandingState) -> Result<()> {
	let pr_url = landing_url(landing_state);
	let gate_view = landing_state.gate_view();

	if pull_request::manual_landing_gates_satisfied(gate_view) {
		return Ok(());
	}
	if gate_view.state != "OPEN" {
		eyre::bail!("Pull request `{pr_url}` is `{}`; adopt requires `OPEN`.", gate_view.state);
	}
	if gate_view.is_draft {
		eyre::bail!("Pull request `{pr_url}` is still draft.");
	}
	if gate_view.pending_review_requests > 0 {
		eyre::bail!(
			"Pull request `{pr_url}` still has {} pending review request(s).",
			gate_view.pending_review_requests
		);
	}
	if gate_view.unresolved_review_threads > 0 {
		eyre::bail!(
			"Pull request `{pr_url}` still has {} unresolved review thread(s).",
			gate_view.unresolved_review_threads
		);
	}
	if gate_view.review_decision == Some("CHANGES_REQUESTED") {
		eyre::bail!("Pull request `{pr_url}` still has active change requests.");
	}

	if let Some(reason) = pull_request::merge_state_requires_review_repair(
		gate_view.mergeable,
		gate_view.merge_state_status,
	) {
		eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.");
	}

	if pull_request::failed_checks_require_repair(
		gate_view.status_check_rollup_state,
		gate_view.merge_state_status,
	) {
		eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.");
	}

	if let Some(other) = gate_view.status_check_rollup_state
		&& pull_request::checks_require_wait(Some(other))
	{
		eyre::bail!(
			"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{other}`."
		);
	}

	if pull_request::mergeability_unknown(gate_view) {
		eyre::bail!("Pull request `{pr_url}` mergeability is still unknown.");
	}
	if !pull_request::merge_state_allows_ready_to_land(gate_view.merge_state_status) {
		eyre::bail!(
			"Pull request `{pr_url}` is not ready to adopt: mergeStateStatus=`{}`.",
			gate_view.merge_state_status
		);
	}
	if gate_view.mergeable != "MERGEABLE" {
		eyre::bail!(
			"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
			gate_view.mergeable
		);
	}

	match gate_view.status_check_rollup_state {
		Some("SUCCESS") | None => Ok(()),
		Some(other) => eyre::bail!(
			"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{other}`."
		),
	}
}

fn validate_adopt_current_worktree(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	landing_state: &PullRequestLandingState,
	cwd: &Path,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<PathBuf> {
	let worktree_path = git_toplevel_path(cwd)?;
	let canonical_worktree = fs::canonicalize(&worktree_path).wrap_err_with(|| {
		format!("Failed to canonicalize current worktree `{}`.", worktree_path.display())
	})?;
	let canonical_root = fs::canonicalize(context.config.worktree_root()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize configured worktree root `{}`.",
			context.config.worktree_root().display()
		)
	})?;

	if !canonical_worktree.starts_with(&canonical_root) || canonical_worktree == canonical_root {
		eyre::bail!(
			"Manual takeover adopt for issue `{}` must run from a managed lane under worktree_root `{}`.",
			issue.identifier,
			context.config.worktree_root().display()
		);
	}

	let local_branch = worktree_checkout_branch_name(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree is detached."))?;

	if let Some(mapping) = existing_worktree_mapping {
		validate_adopt_existing_worktree_mapping(
			context.config.service_id(),
			issue,
			mapping,
			&canonical_worktree,
		)?;
	}

	if local_branch != landing_state.head_ref_name {
		eyre::bail!(
			"Pull request `{}` points at branch `{}`, but current worktree branch is `{local_branch}`.",
			landing_url(landing_state),
			landing_state.head_ref_name
		);
	}
	if !worktree_is_clean(&canonical_worktree)? {
		eyre::bail!(
			"Manual takeover worktree `{}` has local changes; adopt requires a clean lane checkout.",
			canonical_worktree.display()
		);
	}

	let local_head = worktree_head_oid(&canonical_worktree)?
		.ok_or_else(|| eyre::eyre!("Manual takeover worktree has no readable HEAD."))?;

	if landing_state.head_ref_oid != local_head {
		eyre::bail!(
			"Pull request `{}` points at head `{}`, but current worktree HEAD is `{local_head}`.",
			landing_url(landing_state),
			landing_state.head_ref_oid
		);
	}

	Ok(canonical_worktree)
}

fn validate_adopt_existing_worktree_mapping(
	service_id: &str,
	issue: &TrackerIssue,
	mapping: &WorktreeMapping,
	canonical_worktree: &Path,
) -> Result<()> {
	if mapping.project_id() != service_id {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping for project `{}`, not `{}`.",
			issue.identifier,
			mapping.project_id(),
			service_id
		);
	}

	let canonical_mapping = fs::canonicalize(mapping.worktree_path()).wrap_err_with(|| {
		format!(
			"Failed to canonicalize retained worktree mapping `{}` for issue `{}`.",
			mapping.worktree_path().display(),
			issue.identifier
		)
	})?;

	if canonical_mapping != canonical_worktree {
		eyre::bail!(
			"Issue `{}` already has a retained worktree mapping at `{}`, but manual takeover adopt is running from `{}`.",
			issue.identifier,
			mapping.worktree_path().display(),
			canonical_worktree.display()
		);
	}

	Ok(())
}

fn validate_adopt_absent_handoff_marker(
	context: &RecoveryContext,
	issue: &TrackerIssue,
	branch_name: &str,
	existing_worktree_mapping: Option<&WorktreeMapping>,
) -> Result<()> {
	let mut branches = vec![branch_name.to_owned()];

	if let Some(mapping) = existing_worktree_mapping
		&& mapping.branch_name() != branch_name
	{
		branches.push(mapping.branch_name().to_owned());
	}

	for branch in branches {
		if context
			.state_store
			.review_handoff_marker(context.config.service_id(), &issue.id, &branch)?
			.is_some()
		{
			eyre::bail!(
				"Issue `{}` already has a retained review handoff marker for branch `{branch}`; use `decodex land` or `decodex recover review-handoff rebind` instead.",
				issue.identifier
			);
		}
	}

	Ok(())
}

fn apply_review_handoff_rebind(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<()> {
	let handoff_marker = ReviewHandoffMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.worktree.branch_name(),
		landing_url(&validation.landing_state),
		validation.landing_state.base_ref_name.clone(),
		validation.landing_state.head_ref_name.clone(),
		validation.local_head_oid.clone(),
	);
	let orchestration_marker = ReviewOrchestrationMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
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

	if validation.clear_needs_attention_label {
		tracker::set_issue_label_presence(
			&context.tracker,
			&validation.issue,
			context.workflow.frontmatter().tracker().needs_attention_label(),
			false,
		)?;
	}

	if let Some(transition) = validation.success_state_transition.as_ref() {
		context.tracker.update_issue_state(&validation.issue.id, &transition.state_id)?;
	}

	Ok(())
}

fn apply_review_handoff_adopt(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	let handoff_marker = ReviewHandoffMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.branch_name.clone(),
		landing_url(&validation.landing_state),
		validation.landing_state.base_ref_name.clone(),
		validation.landing_state.head_ref_name.clone(),
		validation.local_head_oid.clone(),
	);
	let orchestration_marker = ReviewOrchestrationMarker::new(
		validation.run_id.clone(),
		validation.attempt_number,
		validation.branch_name.clone(),
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
	let worktree_path = validation.worktree_path.to_string_lossy().to_string();
	let local_state_write = context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&validation.issue.id,
			&validation.branch_name,
			&worktree_path,
		)
		.and_then(|()| {
			context.state_store.record_run_attempt(
				&validation.run_id,
				&validation.issue.id,
				validation.attempt_number,
				"starting",
			)
		})
		.and_then(|()| {
			context.state_store.upsert_review_handoff_marker(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
			)
		})
		.and_then(|()| {
			context.state_store.upsert_review_orchestration_marker(
				context.config.service_id(),
				&validation.issue.id,
				&orchestration_marker,
			)
		});

	if let Err(error) = local_state_write {
		mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_markers_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}

	let active_label_restored = match restore_adopt_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			mark_adopt_attempt_failed(context, validation);

			context.state_store.clear_review_markers_for_handoff(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
				&orchestration_marker,
			)?;

			rollback_adopt_worktree_mapping(context, validation)?;

			return Err(error);
		},
	};
	let event = review_handoff_adopt_event(context, validation, active_label_restored);

	if let Err(error) = write_adopt_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_markers_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_adopt_active_label_restoration(context, validation, active_label_restored)?;
		rollback_adopt_worktree_mapping(context, validation)?;

		return Err(error);
	}
	if let Some(transition) = validation.success_state_transition.as_ref() {
		context.tracker.update_issue_state(&validation.issue.id, &transition.state_id)?;
	}

	Ok(())
}

fn restore_adopt_active_label(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<bool> {
	if !validation.should_restore_active_label() {
		return Ok(false);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, true)
}

fn rollback_adopt_active_label_restoration(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	active_label_restored: bool,
) -> Result<()> {
	if !active_label_restored {
		return Ok(());
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, false)?;

	Ok(())
}

fn rollback_adopt_worktree_mapping(
	context: &RecoveryContext,
	validation: &AdoptValidation,
) -> Result<()> {
	if let Some(mapping) = validation.previous_worktree_mapping.as_ref() {
		let worktree_path = mapping.worktree_path().to_string_lossy();

		return context.state_store.upsert_worktree(
			mapping.project_id(),
			mapping.issue_id(),
			mapping.branch_name(),
			&worktree_path,
		);
	}

	context.state_store.clear_worktree(&validation.issue.id)
}

fn mark_adopt_attempt_failed(context: &RecoveryContext, validation: &AdoptValidation) {
	if let Err(error) = context.state_store.update_run_status(&validation.run_id, "failed") {
		tracing::warn!(
			?error,
			run_id = %validation.run_id,
			"Failed to mark manual takeover adopt attempt failed."
		);
	}
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
			run_id: &validation.run_id,
			attempt_number: validation.attempt_number,
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
		"Explicit operator rebind {} for {}.",
		validation.mode.summary_action(),
		validation.issue.identifier,
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("existing_review_handoff_marker={}", validation.mode.evidence_value()),
		format!("needs_attention_label_repair={}", validation.clear_needs_attention_label),
	]);
	event.next_action = Some(String::from("continue retained post-review lifecycle"));

	event
}

fn review_handoff_adopt_event(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	active_label_restored: bool,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
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
		current_timestamp(),
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
		String::from("existing_review_handoff_marker=false"),
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
		"Decodex operator recovery: {} for `{}` to `{}`. This does not land the pull request.",
		validation.mode.summary_action(),
		validation.issue.identifier,
		landing_url(&validation.landing_state)
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;

	tracker::create_prepared_linear_execution_event_comment(
		&context.tracker,
		&validation.issue.id,
		&projection,
	)?;

	Ok(())
}

fn write_adopt_audit(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	event: &LinearExecutionEventRecord,
) -> Result<()> {
	let body = format!(
		"Decodex operator recovery: adopted human-owned PR `{}` for `{}` into retained review handoff state. This does not land the pull request.",
		landing_url(&validation.landing_state),
		validation.issue.identifier,
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;

	tracker::create_prepared_linear_execution_event_comment(
		&context.tracker,
		&validation.issue.id,
		&projection,
	)?;

	Ok(())
}

fn legacy_closeout_event(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
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
		current_timestamp(),
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

fn write_legacy_closeout_audit(
	context: &RecoveryContext,
	validation: &LegacyCloseoutValidation,
	event: &LinearExecutionEventRecord,
) -> Result<bool> {
	let body = format!(
		"Decodex legacy manual closeout audit: verified merged PR `{}` for `{}`. Runtime provenance was `{}`, so this records the manual fallback before local cleanup.",
		landing_url(&validation.landing_state),
		validation.issue.identifier,
		validation.worktree.provenance().source()
	);
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(&body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

fn apply_merged_closeout_recovery(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> Result<(bool, bool)> {
	let closeout_event = merged_closeout_event(context, validation);
	let cleanup_event = merged_closeout_cleanup_event(context, validation);
	let closeout_recorded = write_merged_closeout_event(
		context,
		validation,
		&closeout_event,
		"Decodex merged closeout recovery: verified the PR was merged into the current default branch and reconciled the stale retained attention closeout ledger.",
	)?;
	let cleanup_recorded = match write_merged_closeout_event(
		context,
		validation,
		&cleanup_event,
		"Decodex merged closeout recovery: verified retained lane cleanup is already complete and recorded cleanup_complete.",
	) {
		Ok(cleanup_recorded) => cleanup_recorded,
		Err(error) => {
			if closeout_recorded {
				context
					.state_store
					.forget_linear_execution_event(&closeout_event.idempotency_key)?;
			}

			return Err(error);
		},
	};

	if validation.worktree_mapping.is_some() {
		context.state_store.clear_worktree(&validation.issue.id)?;

		if validation.issue.identifier != validation.issue.id {
			context.state_store.clear_worktree(&validation.issue.identifier)?;
		}
	}

	Ok((closeout_recorded, cleanup_recorded))
}

fn write_merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
	event: &LinearExecutionEventRecord,
	body: &str,
) -> Result<bool> {
	let privacy_classifier = ConfiguredPublicProjectionPrivacyClassifier::from_config(
		context.config.privacy_classifier(),
	)?;
	let projection =
		tracker::prepare_linear_execution_event_comment(body, event, &privacy_classifier)?;
	let recorded = context.state_store.record_linear_execution_event(&projection.record)?;

	if !recorded {
		return Ok(false);
	}

	if let Err(error) = tracker::create_prepared_linear_execution_event_comment_without_remote_scan(
		&context.tracker,
		&validation.issue.id,
		&projection,
	) {
		context.state_store.forget_linear_execution_event(&projection.record.idempotency_key)?;

		return Err(error);
	}

	Ok(true)
}

fn merged_closeout_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
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
		current_timestamp(),
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

fn merged_closeout_cleanup_event(
	context: &RecoveryContext,
	validation: &MergedCloseoutValidation,
) -> LinearExecutionEventRecord {
	let pr_url = landing_url(&validation.landing_state);
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
		timestamp_after_seconds(1),
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
	event.next_action = Some(String::from("No Decodex runtime action remains for this lane."));

	event
}

fn landing_url(landing_state: &PullRequestLandingState) -> &str {
	&landing_state.url
}

fn manual_adopt_run_id(issue_identifier: &str, attempt_number: i64, head_oid: &str) -> String {
	let normalized_issue = issue_identifier
		.chars()
		.map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { '-' })
		.collect::<String>();
	let head_prefix = head_oid.chars().take(12).collect::<String>();

	format!("{normalized_issue}-manual-adopt-{attempt_number}-{head_prefix}")
}

fn current_timestamp() -> String {
	OffsetDateTime::now_utc().format(&Rfc3339).expect("timestamp formatting should succeed")
}

fn timestamp_after_seconds(seconds: i64) -> String {
	(OffsetDateTime::now_utc() + Duration::seconds(seconds))
		.format(&Rfc3339)
		.expect("timestamp formatting should succeed")
}

fn git_toplevel_path(cwd: &Path) -> Result<PathBuf> {
	let output =
		Command::new("git").arg("-C").arg(cwd).args(["rev-parse", "--show-toplevel"]).output()?;

	if output.status.success() {
		return Ok(PathBuf::from(trimmed_stdout(&output.stdout)?));
	}

	let stderr = String::from_utf8_lossy(&output.stderr);

	eyre::bail!(
		"Failed to inspect current Git worktree root from `{}`: {}",
		cwd.display(),
		stderr.trim()
	)
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

fn worktree_head_descends_from_review_handoff(
	worktree_path: &Path,
	recorded_head_oid: &str,
	local_head_oid: &str,
) -> ReviewHandoffLineage {
	if recorded_head_oid == local_head_oid {
		return ReviewHandoffLineage::Descends;
	}

	let Ok(output) = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", recorded_head_oid, local_head_oid])
		.output()
	else {
		return ReviewHandoffLineage::Unknown;
	};

	match output.status.code() {
		Some(0) => ReviewHandoffLineage::Descends,
		Some(1) => ReviewHandoffLineage::Diverged,
		_ => ReviewHandoffLineage::Unknown,
	}
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
		.filter(|line| !state::is_untracked_decodex_runtime_artifact_status_line(line))
		.map(ToOwned::to_owned)
		.collect())
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

fn relative_worktree_path_for_recovery(
	context: &RecoveryContext,
	worktree_path: &Path,
) -> Option<String> {
	repository_relative_path(context.config.repo_root(), worktree_path).or_else(|| {
		worktree_path
			.strip_prefix(context.config.repo_root())
			.ok()
			.map(|relative| relative.to_string_lossy().to_string())
	})
}

#[cfg(test)]
mod tests {
	use std::{fs, path::Path};

	use tempfile::TempDir;

	use crate::{
		pull_request::PullRequestLandingState,
		recovery::{
			REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_BOUND_CLASSIFICATION,
			REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION, REVIEW_HANDOFF_REBIND_EVENT,
			REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
		},
		state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore, WorktreeMapping},
		tracker::{
			TrackerIssue, TrackerState, TrackerTeam,
			records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
		},
		workflow::WorkflowDocument,
	};

	fn sample_worktree(branch_name: &str) -> WorktreeMapping {
		sample_worktree_at(branch_name, Path::new("/tmp/PUB-718"))
	}

	fn sample_worktree_at(branch_name: &str, worktree_path: &Path) -> WorktreeMapping {
		let store = StateStore::open_in_memory().expect("state store should open");
		let worktree_path = worktree_path.to_string_lossy();

		store
			.upsert_worktree("pubfi", "issue-id", branch_name, &worktree_path)
			.expect("worktree should persist");

		store
			.worktree_for_issue("issue-id")
			.expect("worktree should read")
			.expect("worktree should exist")
	}

	fn sample_landing_state(
		pr_url: &str,
		branch_name: &str,
		head_oid: &str,
	) -> PullRequestLandingState {
		PullRequestLandingState {
			url: pr_url.to_owned(),
			state: String::from("OPEN"),
			is_draft: false,
			review_decision: Some(String::from("APPROVED")),
			base_ref_name: String::from("main"),
			pending_review_requests: 0,
			mergeable: String::from("MERGEABLE"),
			merge_state_status: String::from("CLEAN"),
			head_ref_name: branch_name.to_owned(),
			head_ref_oid: head_oid.to_owned(),
			status_check_rollup_state: Some(String::from("SUCCESS")),
			unresolved_review_threads: 0,
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
max_concurrent_agents = 1
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
			identifier: String::from("PUB-718"),
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

	fn run_git(repo: &Path, args: &[&str]) -> String {
		let output = std::process::Command::new("git")
			.arg("-C")
			.arg(repo)
			.args(args)
			.output()
			.expect("git command should run");

		assert!(
			output.status.success(),
			"git {:?} failed: {}",
			args,
			String::from_utf8_lossy(&output.stderr)
		);

		String::from_utf8(output.stdout).expect("git stdout should be utf8").trim().to_owned()
	}

	fn commit_file(repo: &Path, contents: &str) -> String {
		fs::write(repo.join("tracked.txt"), contents).expect("tracked file should write");

		run_git(repo, &["add", "tracked.txt"]);
		run_git(repo, &["commit", "-m", "test commit"]);

		run_git(repo, &["rev-parse", "HEAD"])
	}

	fn temp_git_worktree(branch_name: &str) -> (TempDir, String, String) {
		let temp_dir = TempDir::new().expect("temp git repo should exist");
		let repo = temp_dir.path();

		run_git(repo, &["init"]);
		run_git(repo, &["config", "user.email", "decodex@example.invalid"]);
		run_git(repo, &["config", "user.name", "Decodex Test"]);
		run_git(repo, &["checkout", "-b", branch_name]);

		let first_head = commit_file(repo, "first\n");
		let second_head = commit_file(repo, "second\n");

		(temp_dir, first_head, second_head)
	}

	fn temp_rebased_git_worktree(branch_name: &str) -> (TempDir, String, String) {
		let (temp_dir, first_head, _) = temp_git_worktree(branch_name);
		let repo = temp_dir.path();

		run_git(repo, &["checkout", "--orphan", "rebased"]);
		run_git(repo, &["rm", "-rf", "."]);

		let rebased_head = commit_file(repo, "rebased\n");

		run_git(repo, &["branch", "-D", branch_name]);
		run_git(repo, &["branch", "-m", branch_name]);

		(temp_dir, first_head, rebased_head)
	}

	#[test]
	fn worktree_blocking_status_lines_ignores_untracked_decodex_runtime_artifacts() {
		let (temp_dir, _, _) = temp_git_worktree("x/pubfi-pub-718");
		let repo = temp_dir.path();

		fs::write(repo.join(crate::state::RUN_ACTIVITY_MARKER_FILE), "agent_run\n")
			.expect("activity marker should write");

		let control_dir = repo.join(crate::state::RUN_CONTROL_CHANNEL_DIR);

		fs::create_dir_all(&control_dir).expect("run-control directory should create");
		fs::write(control_dir.join("run-1-1.channel"), "channel\n")
			.expect("run-control channel should write");

		let blocking = super::worktree_blocking_status_lines(repo)
			.expect("worktree status should be readable");

		assert!(blocking.is_empty(), "runtime artifacts should not block rebind: {blocking:?}");
	}

	#[test]
	fn rebind_state_allows_missing_marker_partial_in_progress_handoff() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::RestoreMissingHandoff,
		)
		.expect("missing-marker rebind should recover partial in-progress handoff")
		.expect("partial in-progress handoff should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_allows_current_marker_partial_in_progress_handoff() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::CompleteExistingHandoffState,
		)
		.expect("current-marker state completion should recover partial in-progress handoff")
		.expect("partial in-progress handoff should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_allows_current_marker_failure_state_drift_recovery() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let transition = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::CompleteExistingHandoffState,
		)
		.expect("current-marker state completion should recover failure-state drift")
		.expect("failure-state drift should transition to success state");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");
	}

	#[test]
	fn rebind_state_rejects_failure_state_without_current_marker_repair_mode() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");

		for mode in
			[super::RebindMode::RestoreMissingHandoff, super::RebindMode::RefreshExistingHandoff]
		{
			let error = super::validate_rebind_issue_state_for_policy(
				workflow.frontmatter().tracker(),
				&issue,
				mode,
			)
			.expect_err("failure-state repair requires current-marker completion mode");

			assert!(
				error.to_string().contains("review handoff rebind requires"),
				"unexpected error for {mode:?}: {error}"
			);
		}
	}

	#[test]
	fn rebind_state_requires_success_state_for_existing_marker_refresh() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let error = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			super::RebindMode::RefreshExistingHandoff,
		)
		.expect_err("existing-marker refresh should still require success state");

		assert!(error.to_string().contains("requires `In Review`"));
		assert!(!error.to_string().contains("partial missing-marker"));
	}

	#[test]
	fn adopt_state_allows_in_progress_or_review_only() {
		let workflow = sample_workflow();
		let in_progress = sample_issue("In Progress");
		let transition = super::validate_adopt_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&in_progress,
		)
		.expect("in-progress issue should be adoptable")
		.expect("in-progress issue should transition to review");

		assert_eq!(transition.state_name, "In Review");
		assert_eq!(transition.state_id, "state-review");

		let in_review = sample_issue("In Review");
		let no_transition = super::validate_adopt_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&in_review,
		)
		.expect("in-review issue should remain adoptable");

		assert!(no_transition.is_none());

		let todo = sample_issue("Todo");
		let error =
			super::validate_adopt_issue_state_for_policy(workflow.frontmatter().tracker(), &todo)
				.expect_err("manual takeover should not bypass failure/start states");

		assert!(error.to_string().contains("manual takeover adopt requires"));
	}

	#[test]
	fn adopt_landing_state_rejects_pending_checks() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.status_check_rollup_state = Some(String::from("PENDING"));

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover must not adopt pending checks");

		assert!(error.to_string().contains("still waiting on checks"));
	}

	#[test]
	fn adopt_landing_state_rejects_closed_or_draft_prs() {
		let mut closed = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		closed.state = String::from("CLOSED");

		let error = super::validate_adopt_landing_state(&closed)
			.expect_err("manual takeover must reject closed PRs");

		assert!(error.to_string().contains("adopt requires `OPEN`"));

		let mut draft = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		draft.is_draft = true;

		let error = super::validate_adopt_landing_state(&draft)
			.expect_err("manual takeover must reject draft PRs");

		assert!(error.to_string().contains("is still draft"));
	}

	#[test]
	fn adopt_landing_state_rejects_failed_required_checks() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.status_check_rollup_state = Some(String::from("FAILURE"));
		landing_state.merge_state_status = String::from("BLOCKED");

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover must reject failed required checks");

		assert!(error.to_string().contains("failed required checks"));
	}

	#[test]
	fn adopt_existing_worktree_mapping_accepts_same_project_and_path() {
		let temp_dir = TempDir::new().expect("temp worktree should exist");
		let branch_name = "x/pubfi-pub-718";
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at(branch_name, temp_dir.path());
		let canonical_worktree =
			fs::canonicalize(temp_dir.path()).expect("temp worktree should canonicalize");

		super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&canonical_worktree,
		)
		.expect("matching mapping should be accepted");
	}

	#[test]
	fn adopt_existing_worktree_mapping_accepts_stale_branch_for_same_path() {
		let retained_dir = TempDir::new().expect("retained worktree should exist");
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at("x/pubfi-pub-718-old", retained_dir.path());
		let retained_worktree =
			fs::canonicalize(retained_dir.path()).expect("retained worktree should canonicalize");

		super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&retained_worktree,
		)
		.expect("stale mapping branch should be adopted when path matches");
	}

	#[test]
	fn adopt_existing_worktree_mapping_rejects_stale_path() {
		let retained_dir = TempDir::new().expect("retained worktree should exist");
		let current_dir = TempDir::new().expect("current worktree should exist");
		let issue = sample_issue("In Progress");
		let mapping = sample_worktree_at("x/pubfi-pub-718", retained_dir.path());
		let current_worktree =
			fs::canonicalize(current_dir.path()).expect("current worktree should canonicalize");
		let error = super::validate_adopt_existing_worktree_mapping(
			"pubfi",
			&issue,
			&mapping,
			&current_worktree,
		)
		.expect_err("stale mapping path must be rejected");

		assert!(error.to_string().contains("already has a retained worktree mapping at"));
	}

	#[test]
	fn manual_adopt_run_id_is_stable_for_head() {
		let head_oid = "0123456789abcdef0123456789abcdef01234567";
		let run_id = super::manual_adopt_run_id("XY-944", 2, head_oid);

		assert_eq!(run_id, "xy-944-manual-adopt-2-0123456789ab");
		assert_eq!(run_id, super::manual_adopt_run_id("XY-944", 2, head_oid));
	}

	#[test]
	fn diagnostic_treats_descendant_handoff_head_as_bound() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let (temp_dir, original_head, current_head) = temp_git_worktree(branch_name);
		let worktree = sample_worktree_at(branch_name, temp_dir.path());
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			original_head,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, &current_head);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: None,
			local_branch_name: Some(branch_name),
			local_head_oid: Some(&current_head),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_BOUND_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_record_present");
		assert_eq!(diagnostic.mismatched_field, None);
	}

	#[test]
	fn diagnostic_requires_rebind_when_current_marker_state_transition_pending() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Progress",
			success_state: "In Review",
			in_progress_state: "In Progress",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_state_transition_pending");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("pending issue-state transition"));
	}

	#[test]
	fn diagnostic_requires_refresh_when_handoff_head_is_stale() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let (temp_dir, original_head, rebased_head) = temp_rebased_git_worktree(branch_name);
		let worktree = sample_worktree_at(branch_name, temp_dir.path());
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			original_head,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, &rebased_head);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: None,
			local_branch_name: Some(branch_name),
			local_head_oid: Some(&rebased_head),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_handoff_lineage_mismatch");
		assert_eq!(diagnostic.pr_head_oid.as_deref(), Some(rebased_head.as_str()));
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_handoff.pr_head_oid"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
	}

	#[test]
	fn diagnostic_requires_refresh_when_orchestration_head_is_stale() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"0123456789abcdef0123456789abcdef01234567",
			"waiting_for_result",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(true),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "review_orchestration_head_mismatch");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_orchestration.head_sha"));
	}

	#[test]
	fn diagnostic_bound_handoff_reports_missing_active_ownership_recovery() {
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let diagnostic = super::diagnostic_binding(super::HandoffDiagnosticRequest {
			service_id: "pubfi",
			issue_identifier: "PUB-718",
			issue_state_name: "In Review",
			success_state: "In Review",
			in_progress_state: "In Progress",
			worktree: &worktree,
			existing_handoff: Some(&handoff),
			existing_orchestration: Some(&orchestration),
			local_branch_name: Some(branch_name),
			local_head_oid: Some(head_oid),
			worktree_clean: Some(true),
			pr_inspection: Some(&landing_state),
			active_label_present: Some(false),
		});

		assert_eq!(diagnostic.classification, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION);
		assert_eq!(diagnostic.reason, "active_ownership_label_missing");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.labels"));
		assert!(diagnostic.next_action.contains("decodex:active:pubfi"));
		assert!(diagnostic.next_action.contains("Restore explicit lane ownership"));
	}

	#[test]
	fn rebind_validation_refreshes_existing_same_branch_pr_marker() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Review");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			"0123456789abcdef0123456789abcdef01234567",
		);
		let landing_state =
			sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			None,
			&landing_state,
			"1123456789abcdef0123456789abcdef01234567",
		)
		.expect("stale existing marker should be refreshable");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::RefreshExistingHandoff);
	}

	#[test]
	fn rebind_validation_rejects_stale_marker_failure_state_drift_recovery() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			"0123456789abcdef0123456789abcdef01234567",
		);
		let landing_state =
			sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
		let (_run_id, _attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			None,
			&landing_state,
			"1123456789abcdef0123456789abcdef01234567",
		)
		.expect("stale existing marker should require marker refresh first");

		assert_eq!(mode, super::RebindMode::RefreshExistingHandoff);

		let error = super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			mode,
		)
		.expect_err("stale marker refresh must not repair failure-state drift");

		assert!(error.to_string().contains("review handoff rebind requires"));
	}

	#[test]
	fn rebind_validation_rejects_current_existing_marker_as_noop() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Review");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let error = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect_err("current existing marker should not be rebound");

		assert!(error.to_string().contains("no rebind is needed"));
	}

	#[test]
	fn rebind_validation_completes_current_existing_marker_state_transition() {
		let workflow = sample_workflow();
		let issue = sample_issue("In Progress");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect("current marker should allow state-only handoff completion");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::CompleteExistingHandoffState);
	}

	#[test]
	fn rebind_validation_completes_current_existing_marker_failure_state_drift() {
		let workflow = sample_workflow();
		let issue = sample_issue("Todo");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let worktree = sample_worktree(branch_name);
		let handoff = ReviewHandoffMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			"main",
			branch_name,
			head_oid,
		);
		let orchestration = ReviewOrchestrationMarker::new(
			"pub-718-attempt-1",
			1,
			branch_name,
			pr_url,
			head_oid,
			"request_pending",
			None,
			None,
			None,
			0,
			0,
			None,
		);
		let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
		let (run_id, attempt_number, mode) = super::validate_existing_handoff_refresh(
			workflow.frontmatter().tracker(),
			&issue,
			&worktree,
			&handoff,
			Some(&orchestration),
			&landing_state,
			head_oid,
		)
		.expect("current marker should allow failure-state drift completion");

		assert_eq!(run_id, "pub-718-attempt-1");
		assert_eq!(attempt_number, 1);
		assert_eq!(mode, super::RebindMode::CompleteExistingHandoffState);
	}

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
	fn review_handoff_adopt_event_validation_accepts_required_fields() {
		let mut record = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "decodex",
				issue_id: "issue-id",
				issue_identifier: "XY-944",
				run_id: "xy-944-manual-adopt-1",
				attempt_number: 1,
			},
			REVIEW_HANDOFF_ADOPT_EVENT,
			super::current_timestamp(),
			"anchor",
		);

		record.branch = Some(String::from("xy/xy-944-manual-takeover-adopt"));
		record.worktree_path = Some(String::from(".worktrees/XY-944"));
		record.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/344"));
		record.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.pr_base_ref = Some(String::from("main"));
		record.commit_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		record.validation_result = Some(String::from("passed"));
		record.summary =
			Some(String::from("Explicit operator manual takeover adopted review handoff."));
		record.evidence = Some(vec![String::from("manual_takeover_adopt=true")]);

		records::validate_linear_execution_event_record(&record)
			.expect("adopt event should validate");
	}

	#[test]
	fn merged_closeout_recovery_events_validate() {
		let mut closeout = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi-mono",
				issue_id: "issue-id",
				issue_identifier: "PUB-1549",
				run_id: "pub-1549-attempt-1-1781240781",
				attempt_number: 1,
			},
			super::LEGACY_MANUAL_CLOSEOUT_EVENT,
			super::current_timestamp(),
			"anchor-closeout",
		);

		closeout.branch = Some(String::from("x/pubfi-mono-pub-1549"));
		closeout.worktree_path = Some(String::from(".worktrees/PUB-1549"));
		closeout.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
		closeout.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		closeout.pr_base_ref = Some(String::from("main"));
		closeout.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
		closeout.validation_result = Some(String::from("passed"));
		closeout.target_state = Some(String::from("Done"));
		closeout.summary = Some(String::from("Merged closeout recovery recorded."));

		records::validate_linear_execution_event_record(&closeout)
			.expect("merged closeout event should validate");

		let mut cleanup = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi-mono",
				issue_id: "issue-id",
				issue_identifier: "PUB-1549",
				run_id: "pub-1549-attempt-1-1781240781",
				attempt_number: 1,
			},
			"cleanup_complete",
			super::timestamp_after_seconds(1),
			"anchor-cleanup",
		);

		cleanup.branch = Some(String::from("x/pubfi-mono-pub-1549"));
		cleanup.worktree_path = Some(String::from(".worktrees/PUB-1549"));
		cleanup.pr_url = Some(String::from("https://github.com/helixbox/pubfi-mono/pull/309"));
		cleanup.pr_head_sha = Some(String::from("0123456789abcdef0123456789abcdef01234567"));
		cleanup.pr_base_ref = Some(String::from("main"));
		cleanup.commit_sha = Some(String::from("1123456789abcdef0123456789abcdef01234567"));
		cleanup.cleanup_status = Some(String::from("merged_closeout_reconciled"));
		cleanup.target_state = Some(String::from("Done"));
		cleanup.summary = Some(String::from("Merged closeout recovery marked cleanup complete."));

		records::validate_linear_execution_event_record(&cleanup)
			.expect("merged closeout cleanup event should validate");
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
