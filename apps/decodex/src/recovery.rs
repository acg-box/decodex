//! Explicit operator recovery surfaces for retained Decodex lanes.

use std::{
	collections::HashMap,
	env, fs,
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::Report;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	config::ServiceConfig,
	github,
	prelude::{Result, eyre},
	pull_request::PullRequestLandingState,
	runtime,
	state::{
		ConnectorBackoffInput, RUN_ACTIVITY_MARKER_FILE, ReviewHandoffMarker,
		ReviewOrchestrationMarker, StateStore, WorktreeMapping,
	},
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::WorkflowDocument,
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_BOUND_CLASSIFICATION: &str = "review_handoff_bound";
const REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION: &str = "review_handoff_ownership_drift";
const REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION: &str = "review_handoff_rebind_required";
const REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION: &str = "review_handoff_unverified";
const REVIEW_HANDOFF_MISMATCH_CLASSIFICATION: &str = "review_handoff_mismatch";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RebindMode {
	RestoreMissingHandoff,
	RefreshExistingHandoff,
}
impl RebindMode {
	fn evidence_value(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "absent",
			Self::RefreshExistingHandoff => "refreshed",
		}
	}

	fn summary_verb(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "restored",
			Self::RefreshExistingHandoff => "refreshed",
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
		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} mode={} active_label_present={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.mode.evidence_value(),
			validation.active_label_present
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
		"Inspect PR lineage, ensure label `{}` is present, then run `decodex recover review-handoff rebind {} --pr <URL>` if the PR exactly matches this retained lane.",
		tracker::automation_active_label(service_id),
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
	let active_label_present = validate_rebind_tracker_labels(context, &issue)?;
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
		active_label_present,
		mode,
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

	Ok(worktree)
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
		&issue.identifier,
		worktree,
		existing_handoff,
		existing_orchestration,
		landing_state,
		local_head_oid,
	)
}

fn validate_existing_handoff_refresh(
	issue_identifier: &str,
	worktree: &WorktreeMapping,
	existing_handoff: &ReviewHandoffMarker,
	existing_orchestration: Option<&ReviewOrchestrationMarker>,
	landing_state: &PullRequestLandingState,
	local_head_oid: &str,
) -> Result<(String, i64, RebindMode)> {
	if existing_handoff.pr_url() != landing_url(landing_state) {
		eyre::bail!(
			"Issue `{}` already has review handoff marker for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
			issue_identifier,
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
		eyre::bail!(
			"Issue `{}` already has a review handoff marker for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
			issue_identifier,
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
		"Explicit operator rebind {} retained review handoff marker for {}.",
		validation.mode.summary_verb(),
		validation.issue.identifier,
	));
	event.evidence = Some(vec![
		format!("issue_state={}", validation.issue.state.name),
		format!("branch={}", validation.worktree.branch_name()),
		format!("pr_url={pr_url}"),
		format!("pr_head_sha={}", validation.local_head_oid),
		format!("existing_review_handoff_marker={}", validation.mode.evidence_value()),
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
		"Decodex operator recovery: {} retained review handoff marker for `{}` to `{}`. This does not land the pull request.",
		validation.mode.summary_verb(),
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
	use std::{fs, path::Path};

	use tempfile::TempDir;

	use crate::{
		pull_request::PullRequestLandingState,
		recovery::{
			REVIEW_HANDOFF_BOUND_CLASSIFICATION, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION,
			REVIEW_HANDOFF_REBIND_EVENT, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
		},
		state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore, WorktreeMapping},
		tracker::records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
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
			"PUB-718",
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
	fn rebind_validation_rejects_current_existing_marker_as_noop() {
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
			"PUB-718",
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
