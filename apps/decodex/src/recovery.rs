//! Explicit operator recovery surfaces for retained Decodex lanes.

use std::{
	collections::{BTreeSet, HashMap, HashSet},
	env, fs,
	path::{Path, PathBuf},
	process::Command,
};

use color_eyre::{Report, eyre::WrapErr};
use serde::Serialize;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	commit_message,
	config::ServiceConfig,
	github, orchestrator,
	prelude::{Result, eyre},
	pull_request::{self, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState},
	runtime,
	state::{
		self, ConnectorBackoffInput, PrivateExecutionEvent, ProjectRunStatus,
		RUN_CONTROL_CHANNEL_STATUS_ACTIVE, RUN_CONTROL_CHANNEL_STATUS_FAILED, ReviewHandoffMarker,
		ReviewOrchestrationMarker, StateStore, WorktreeMapping,
	},
	tracker::{
		self, IssueTracker, TrackerIssue,
		linear::LinearClient,
		privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier,
		records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
	},
	workflow::{WorkflowDocument, WorkflowTracker},
	worktree::WorktreeManager,
};

const MISSING_HANDOFF_REASON: &str = "missing_review_handoff_record";
const ORPHANED_REVIEW_HANDOFF_CLASSIFICATION: &str = "orphaned_review_handoff";
const REVIEW_HANDOFF_BOUND_CLASSIFICATION: &str = "review_handoff_bound";
const REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION: &str = "review_handoff_ownership_drift";
const REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION: &str = "review_handoff_rebind_required";
const REVIEW_HANDOFF_UNVERIFIED_CLASSIFICATION: &str = "review_handoff_unverified";
const REVIEW_HANDOFF_MISMATCH_CLASSIFICATION: &str = "review_handoff_mismatch";
const REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION: &str = "stale_terminal_local_residue";
const REVIEW_HANDOFF_REBIND_EVENT: &str = "review_handoff_rebind";
const REVIEW_HANDOFF_ADOPT_EVENT: &str = "review_handoff_adopt";
const LEGACY_MANUAL_CLOSEOUT_EVENT: &str = "closeout";
const LEGACY_MANUAL_CLOSEOUT_ANCHOR: &str = "legacy_manual_closeout";
const MERGED_CLOSEOUT_CLOSEOUT_ANCHOR: &str = "merged_closeout";
const MERGED_CLOSEOUT_CLEANUP_ANCHOR: &str = "merged_closeout_cleanup";
const GHOST_LANE_CLASSIFICATION: &str = "missing_issue_ghost_lane";
const MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION: &str = "mcp_test_fixture_ghost_lane";
const GHOST_LANE_BLOCKED_CLASSIFICATION: &str = "ghost_lane_recovery_blocked";
const GHOST_LANE_CLEANUP_EVENT: &str = "ghost_lane_cleanup";
const GHOST_LANE_TERMINAL_STATUS: &str = "terminal_guarded";
const STALE_ACTIVE_CLASSIFICATION: &str = "stale_active_ownership";
const STALE_ACTIVE_BLOCKED_CLASSIFICATION: &str = "stale_active_recovery_blocked";
const STALE_ACTIVE_RELEASE_EVENT: &str = "stale_active_release";
const STALE_ACTIVE_RECOVERY_SCHEMA: &str = "decodex.stale_active_recovery_private_event/1";
const MCP_TEST_FIXTURE_SOURCE: &str = "mcp-test";
const MCP_TEST_FIXTURE_PROJECT_ID: &str = "pubfi";
const MCP_TEST_FIXTURE_ISSUE_ID: &str = "PUB-012";
const MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER: &str = "PUBFI-012";
const MCP_TEST_FIXTURE_RUN_ID: &str = "run-12";
const MCP_TEST_FIXTURE_THREAD_ID: &str = "thread-12";
const MCP_TEST_FIXTURE_TURN_ID: &str = "turn-12";
const REBOUND_ORCHESTRATION_PHASE: &str = "request_pending";
const LINEAR_RATE_LIMIT_BACKOFF_WARNING: &str = "tracker_rate_limited";
const LINEAR_RATE_LIMIT_BACKOFF_SECS: i64 = 15 * 60;
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING: &str = "tracker_transient_timeout";
const LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS: i64 = 60;

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
	/// Validate without writing a lifecycle record or tracker audit comments.
	pub(crate) dry_run: bool,
}

/// Explicit manual PR takeover into retained review handoff state.
#[derive(Debug)]
pub(crate) struct ReviewHandoffAdoptRequest {
	/// Issue identifier to adopt.
	pub(crate) issue: String,
	/// Pull request URL to adopt.
	pub(crate) pr_url: String,
	/// Validate without writing runtime lifecycle state or tracker audit comments.
	pub(crate) dry_run: bool,
}

/// Read-only ghost-lane diagnostic request.
#[derive(Debug)]
pub(crate) struct GhostLaneDiagnoseRequest {
	/// Optional issue identifier or local issue id to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit missing-issue ghost-lane cleanup request.
#[derive(Debug)]
pub(crate) struct GhostLaneCleanupRequest {
	/// Issue identifier or local issue id to terminalize.
	pub(crate) issue: String,
	/// Validate without writing runtime state.
	pub(crate) dry_run: bool,
}

/// Read-only tracker-present stale active ownership diagnostic request.
#[derive(Debug)]
pub(crate) struct StaleActiveDiagnoseRequest {
	/// Optional issue identifier or tracker issue id to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit tracker-present stale active ownership release request.
#[derive(Debug)]
pub(crate) struct StaleActiveReleaseRequest {
	/// Issue identifier or tracker issue id to release.
	pub(crate) issue: String,
	/// Validate without mutating tracker labels or runtime state.
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
	existing_lifecycle_handoff_head_oid: Option<String>,
	existing_lifecycle_phase_head_oid: Option<String>,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
	mismatched_field: Option<String>,
	active_label_present: Option<bool>,
	next_action: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct GhostLaneRecoveryReport {
	project_id: String,
	diagnostics: Vec<GhostLaneDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct GhostLaneDiagnostic {
	project_id: String,
	issue_id: String,
	issue_identifier: Option<String>,
	run_id: String,
	attempt_number: i64,
	attempt_status: String,
	classification: String,
	reason: String,
	run_lease: bool,
	control_channel: String,
	evidence: Vec<String>,
	blockers: Vec<String>,
	next_action: String,
}
impl GhostLaneDiagnostic {
	fn recoverable(&self) -> bool {
		(self.classification == GHOST_LANE_CLASSIFICATION
			|| self.classification == MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION)
			&& self.blockers.is_empty()
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct StaleActiveRecoveryReport {
	project_id: String,
	diagnostics: Vec<StaleActiveDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct StaleActiveDiagnostic {
	project_id: String,
	issue_id: String,
	issue_identifier: String,
	issue_state: String,
	classification: String,
	reason: String,
	queue_label_present: bool,
	active_label_present: bool,
	needs_attention_label_present: bool,
	latest_run_id: Option<String>,
	latest_attempt_number: Option<i64>,
	latest_attempt_status: Option<String>,
	run_lease: bool,
	active_shared_claim: bool,
	control_channel: String,
	worktree_path: Option<String>,
	worktree_state: String,
	evidence: Vec<String>,
	blockers: Vec<String>,
	next_action: String,
}
impl StaleActiveDiagnostic {
	fn recoverable(&self) -> bool {
		self.classification == STALE_ACTIVE_CLASSIFICATION && self.blockers.is_empty()
	}
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
	failure_state: &'a str,
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
	runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecoveryRuntimeMutationPolicy {
	AllowRuntimeWrites,
	ReadOnly,
}
impl RecoveryRuntimeMutationPolicy {
	const fn allows_runtime_writes(self) -> bool {
		matches!(self, Self::AllowRuntimeWrites)
	}
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
	restore_active_label: bool,
	mode: RebindMode,
	success_state_transition: Option<RebindSuccessStateTransition>,
	clear_needs_attention_label: bool,
}
impl RebindValidation {
	fn should_restore_active_label(&self) -> bool {
		self.restore_active_label
	}
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
	restore_active_label: bool,
	clear_needs_attention_label: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RebindMode {
	RestoreMissingHandoff,
	RefreshExistingHandoff,
	CompleteExistingHandoffState,
}
impl RebindMode {
	fn as_str(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "restore_missing_handoff",
			Self::RefreshExistingHandoff => "refresh_existing_handoff",
			Self::CompleteExistingHandoffState => "complete_existing_handoff_state",
		}
	}

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
			Self::RestoreMissingHandoff => "restored retained review lifecycle record",
			Self::RefreshExistingHandoff => "refreshed retained review lifecycle record",
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
	let context = load_recovery_context_read_only(config_path)?;

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
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
	let validation = validate_rebind_request(&context, request)?;

	if request.dry_run {
		let state_transition = validation
			.success_state_transition
			.as_ref()
			.map_or("none", |transition| transition.state_name.as_str());

		println!(
			"dry run: review handoff rebind validated for project={} issue={} branch={} pr={} head={} mode={} active_label_present={} would_restore_active_label={} state_transition={}",
			context.config.service_id(),
			validation.issue.identifier,
			validation.worktree.branch_name(),
			landing_url(&validation.landing_state),
			validation.local_head_oid,
			validation.mode.evidence_value(),
			validation.active_label_present,
			validation.should_restore_active_label(),
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
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
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

/// Run a read-only missing-issue ghost-lane diagnostic.
pub(crate) fn run_ghost_lane_diagnose(
	config_path: Option<&Path>,
	request: &GhostLaneDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context_read_only(config_path)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match diagnose_ghost_lanes_read_only(
		context.config.service_id(),
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		request.issue.as_deref(),
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};

	if let Err(error) = apply_ghost_lane_live_status_blockers(&context, &mut diagnostics) {
		if let Some(message) =
			remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
		{
			println!("{message}");

			return Ok(());
		}

		return Err(error);
	}

	let report =
		GhostLaneRecoveryReport { project_id: context.config.service_id().to_owned(), diagnostics };

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_ghost_lane_recovery_report(&report));
	}

	Ok(())
}

/// Terminalize a proven missing-issue ghost lane and clear its local run lease.
pub(crate) fn run_ghost_lane_cleanup(
	config_path: Option<&Path>,
	request: &GhostLaneCleanupRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match if context.runtime_mutation_policy.allows_runtime_writes() {
		diagnose_ghost_lanes(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&context.tracker,
			Some(&request.issue),
		)
	} else {
		diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&context.tracker,
			Some(&request.issue),
		)
	} {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let diagnostic = diagnostics
		.pop()
		.ok_or_else(|| eyre::eyre!("No leased lane matched `{}`.", request.issue))?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover ghost-lane cleanup` refused `{}` because safety inspection reported blockers: {}",
			request.issue,
			diagnostic.blockers.join(", ")
		);
	}

	if let Err(error) = ensure_ghost_lane_live_status_allows_cleanup(&context, &diagnostic) {
		if let Some(message) =
			remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
		{
			println!("{message}");

			return Ok(());
		}

		return Err(error);
	}

	if request.dry_run {
		println!(
			"dry run: ghost lane cleanup validated for project={} issue={} run_id={} attempt={} classification={}",
			diagnostic.project_id,
			render_ghost_lane_issue(&diagnostic),
			diagnostic.run_id,
			diagnostic.attempt_number,
			diagnostic.classification
		);

		return Ok(());
	}

	apply_ghost_lane_cleanup(&context.state_store, &diagnostic)?;

	println!(
		"ghost lane cleanup ok: project={} issue={} run_id={} attempt={} status={} lease_cleared=yes",
		diagnostic.project_id,
		render_ghost_lane_issue(&diagnostic),
		diagnostic.run_id,
		diagnostic.attempt_number,
		GHOST_LANE_TERMINAL_STATUS
	);

	Ok(())
}

/// Run a read-only tracker-present stale active ownership diagnostic.
pub(crate) fn run_stale_active_diagnose(
	config_path: Option<&Path>,
	request: &StaleActiveDiagnoseRequest,
) -> Result<()> {
	let context = load_recovery_context_read_only(config_path)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let diagnostics = match diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		request.issue.as_deref(),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "stale_active_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let report = StaleActiveRecoveryReport {
		project_id: context.config.service_id().to_owned(),
		diagnostics,
	};

	if request.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		print!("{}", render_stale_active_recovery_report(&report));
	}

	Ok(())
}

/// Release a tracker-present stale active ownership label after fail-closed checks.
pub(crate) fn run_stale_active_release(
	config_path: Option<&Path>,
	request: &StaleActiveReleaseRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;

	if let Some(message) = active_recovery_tracker_backoff_message(&context)? {
		println!("{message}");

		return Ok(());
	}

	let mut diagnostics = match diagnose_stale_active_issues(
		context.config.service_id(),
		&context.workflow,
		context.config.worktree_root(),
		&context.state_store,
		&context.tracker,
		Some(&request.issue),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	) {
		Ok(diagnostics) => diagnostics,
		Err(error) => {
			if let Some(message) =
				remember_recovery_tracker_backoff_message(&context, &error, "stale_active_recovery")
			{
				println!("{message}");

				return Ok(());
			}

			return Err(error);
		},
	};
	let diagnostic = diagnostics
		.pop()
		.ok_or_else(|| eyre::eyre!("No stale active issue matched `{}`.", request.issue))?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because safety inspection reported blockers: {}",
			request.issue,
			diagnostic.blockers.join(", ")
		);
	}

	preflight_stale_active_worktree_cleanup(&context.state_store, &diagnostic)?;

	if request.dry_run {
		println!(
			"dry run: stale active release validated for project={} issue={} run_id={} attempt={} classification={}",
			diagnostic.project_id,
			diagnostic.issue_identifier,
			diagnostic.latest_run_id.as_deref().unwrap_or("none"),
			diagnostic
				.latest_attempt_number
				.map(|attempt| attempt.to_string())
				.unwrap_or_else(|| String::from("none")),
			diagnostic.classification
		);

		return Ok(());
	}

	apply_stale_active_release(&context, &diagnostic)?;

	println!(
		"stale active release ok: project={} issue={} active_label_released=yes queue_label_preserved={} terminal_status={}",
		diagnostic.project_id,
		diagnostic.issue_identifier,
		diagnostic.queue_label_present,
		GHOST_LANE_TERMINAL_STATUS
	);

	Ok(())
}

/// Run an explicit audited legacy closeout fallback.
pub(crate) fn run_legacy_closeout(
	config_path: Option<&Path>,
	request: &LegacyCloseoutRecoveryRequest,
) -> Result<()> {
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
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
	let context = load_recovery_context_for_dry_run(config_path, request.dry_run)?;
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

fn load_recovery_context_read_only(config_path: Option<&Path>) -> Result<RecoveryContext> {
	load_recovery_context_with_policy(config_path, RecoveryRuntimeMutationPolicy::ReadOnly)
}

fn load_recovery_context_for_dry_run(
	config_path: Option<&Path>,
	dry_run: bool,
) -> Result<RecoveryContext> {
	let runtime_mutation_policy = if dry_run {
		RecoveryRuntimeMutationPolicy::ReadOnly
	} else {
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites
	};

	load_recovery_context_with_policy(config_path, runtime_mutation_policy)
}

fn load_recovery_context_with_policy(
	config_path: Option<&Path>,
	runtime_mutation_policy: RecoveryRuntimeMutationPolicy,
) -> Result<RecoveryContext> {
	let state_store = runtime::open_runtime_store()?;
	let config_path = resolve_recovery_config_path(config_path, &state_store)?;
	let config = ServiceConfig::from_path(&config_path)?;
	let workflow = WorkflowDocument::from_path(config.workflow_path())?;
	let tracker = LinearClient::new(config.tracker().resolve_api_key()?)?;

	if runtime_mutation_policy.allows_runtime_writes() {
		runtime::register_project_config(&state_store, &config_path, true)?;
	}
	state_store.observe_dispatch_slot_root(config.service_id(), config.worktree_root())?;

	Ok(RecoveryContext { config, workflow, state_store, tracker, runtime_mutation_policy })
}

fn active_recovery_tracker_backoff_message(context: &RecoveryContext) -> Result<Option<String>> {
	let Some(backoff) =
		context.state_store.connector_backoff(context.config.service_id(), "linear")?
	else {
		return Ok(None);
	};
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();

	if backoff.reset_unix_epoch() <= now_unix_epoch {
		if context.runtime_mutation_policy.allows_runtime_writes() {
			context.state_store.clear_connector_backoff(context.config.service_id(), "linear")?;
		}

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
	let now_unix_epoch = OffsetDateTime::now_utc().unix_timestamp();
	let (quota_class, reset_unix_epoch, reset_source, warning) = if message
		.contains("Linear connector is rate limited")
	{
		let (reset_unix_epoch, reset_source) =
			match parse_recovery_rate_limit_reset_unix_epoch(&message) {
				Some(reset) if reset > now_unix_epoch => (reset, "linear"),
				_ =>
					(now_unix_epoch.saturating_add(LINEAR_RATE_LIMIT_BACKOFF_SECS), "local_default"),
			};

		(
			"linear_graphql_rate_limit",
			reset_unix_epoch,
			reset_source,
			LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		)
	} else if message.contains("Linear connector timed out") {
		(
			"linear_graphql_timeout",
			now_unix_epoch.saturating_add(LINEAR_TRANSIENT_TIMEOUT_BACKOFF_SECS),
			"local_transient_timeout",
			LINEAR_TRANSIENT_TIMEOUT_BACKOFF_WARNING,
		)
	} else {
		return None;
	};

	if !context.runtime_mutation_policy.allows_runtime_writes() {
		return Some(recovery_tracker_backoff_message(
			context.config.service_id(),
			sync_phase,
			reset_unix_epoch,
			reset_unix_epoch.saturating_sub(now_unix_epoch),
		));
	}

	if let Err(store_error) = context.state_store.upsert_connector_backoff(ConnectorBackoffInput {
		project_id: context.config.service_id(),
		connector: "linear",
		sync_phase,
		quota_class,
		reset_unix_epoch,
		reset_source,
		warning,
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
	diagnose_all_retained_review_worktrees_with_tracker(context, &context.tracker)
}

fn diagnose_all_retained_review_worktrees_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
) -> Result<Vec<ReviewHandoffDiagnostic>>
where
	T: IssueTracker,
{
	let mut worktrees = Vec::new();
	let mut diagnostics = Vec::new();

	for worktree in context.state_store.list_worktrees(context.config.service_id())? {
		if retained_review_worktree_is_stale_terminal_residue(context, &worktree)? {
			diagnostics.push(stale_terminal_residue_review_handoff_diagnostic(context, &worktree));
		} else {
			worktrees.push(worktree);
		}
	}

	let issues_by_id = refresh_retained_review_worktree_issues(tracker, &worktrees)?;
	let tracker_policy = context.workflow.frontmatter().tracker();
	let success_state = tracker_policy.success_state();
	let in_progress_state = tracker_policy.in_progress_state();

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

fn retained_review_worktree_is_stale_terminal_residue(
	context: &RecoveryContext,
	worktree: &WorktreeMapping,
) -> Result<bool> {
	let active_issue_ids = context
		.state_store
		.list_active_shared_leases(context.config.service_id())?
		.into_iter()
		.map(|lease| lease.issue_id().to_owned())
		.collect::<HashSet<_>>();

	orchestrator::worktree_mapping_is_stale_terminal_local_residue(
		&context.config,
		&context.state_store,
		worktree,
		&active_issue_ids,
	)
}

fn stale_terminal_residue_review_handoff_diagnostic(
	context: &RecoveryContext,
	worktree: &WorktreeMapping,
) -> ReviewHandoffDiagnostic {
	ReviewHandoffDiagnostic {
		project_id: context.config.service_id().to_owned(),
		issue_id: worktree.issue_id().to_owned(),
		issue_identifier: worktree.issue_id().to_owned(),
		issue_state: String::from("local_terminal_residue"),
		classification: String::from(REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION),
		reason: String::from(
			"terminal_unleased_runtime_recorded_identifier_mapping_with_missing_path",
		),
		branch_name: worktree.branch_name().to_owned(),
		worktree_path: worktree.worktree_path().display().to_string(),
		local_branch_name: None,
		local_head_oid: None,
		worktree_clean: None,
		existing_pr_url: None,
		existing_lifecycle_handoff_head_oid: None,
		existing_lifecycle_phase_head_oid: None,
		pr_base_ref: None,
		pr_head_oid: None,
		mismatched_field: None,
		active_label_present: None,
		next_action: String::from(
			"No review-handoff recovery action is required; project reconciliation clears this stale local mapping before tracker refresh.",
		),
	}
}

fn refresh_retained_review_worktree_issues<T>(
	tracker: &T,
	worktrees: &[WorktreeMapping],
) -> Result<HashMap<String, TrackerIssue>>
where
	T: IssueTracker,
{
	if worktrees.is_empty() {
		return Ok(HashMap::new());
	}

	let issue_ids =
		worktrees.iter().map(|worktree| worktree.issue_id().to_owned()).collect::<Vec<_>>();

	Ok(tracker
		.refresh_issues(&issue_ids)?
		.into_iter()
		.map(|issue| (issue.id.clone(), issue))
		.collect())
}

fn diagnose_issue(
	context: &RecoveryContext,
	issue_identifier: &str,
) -> Result<ReviewHandoffDiagnostic> {
	diagnose_issue_with_tracker(context, &context.tracker, issue_identifier)
}

fn diagnose_issue_with_tracker<T>(
	context: &RecoveryContext,
	tracker: &T,
	issue_identifier: &str,
) -> Result<ReviewHandoffDiagnostic>
where
	T: IssueTracker,
{
	if let Some(worktree) = stale_terminal_residue_worktree_for_issue(context, issue_identifier)? {
		return Ok(stale_terminal_residue_review_handoff_diagnostic(context, &worktree));
	}

	let issue = load_issue_by_identifier(tracker, issue_identifier)?;
	let worktree = context.state_store.worktree_for_issue(&issue.id)?.ok_or_else(|| {
		eyre::eyre!("Issue `{}` has no retained worktree mapping.", issue.identifier)
	})?;

	diagnose_issue_worktree(context, issue, worktree)
}

fn stale_terminal_residue_worktree_for_issue(
	context: &RecoveryContext,
	issue_identifier: &str,
) -> Result<Option<WorktreeMapping>> {
	let Some(worktree) = context.state_store.worktree_for_issue(issue_identifier)? else {
		return Ok(None);
	};

	if retained_review_worktree_is_stale_terminal_residue(context, &worktree)? {
		Ok(Some(worktree))
	} else {
		Ok(None)
	}
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
		failure_state: context.workflow.frontmatter().tracker().failure_state(),
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
		existing_lifecycle_handoff_head_oid: existing_handoff
			.as_ref()
			.map(|handoff| handoff.pr_head_oid().to_owned()),
		existing_lifecycle_phase_head_oid: existing_orchestration
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

	if let Some(diagnostic) = handoff_issue_state_drift_diagnostic(
		&request,
		existing_handoff,
		pr_base_ref.clone(),
		pr_head_oid.clone(),
	) {
		return diagnostic;
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

fn handoff_issue_state_drift_diagnostic(
	request: &HandoffDiagnosticRequest<'_>,
	existing_handoff: &ReviewHandoffMarker,
	pr_base_ref: Option<String>,
	pr_head_oid: Option<String>,
) -> Option<HandoffBindingDiagnostic> {
	if request.active_label_present == Some(false) {
		let next_action = if request.issue_state_name == request.in_progress_state
			|| request.issue_state_name == request.failure_state
		{
			rebind_state_transition_next_action(request.issue_identifier, existing_handoff.pr_url())
		} else if request.issue_state_name == request.success_state {
			bound_handoff_next_action(request.service_id, request.active_label_present)
		} else {
			issue_state_mismatch_next_action(request.success_state, request.in_progress_state)
		};

		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION),
			reason: String::from("active_ownership_label_missing"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.labels")),
			next_action,
		});
	}

	if request.issue_state_name == request.in_progress_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_state_transition_pending"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}

	if request.issue_state_name == request.failure_state {
		return Some(HandoffBindingDiagnostic {
			classification: String::from(REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION),
			reason: String::from("review_handoff_failure_state_drift"),
			pr_base_ref,
			pr_head_oid,
			mismatched_field: Some(String::from("issue.state")),
			next_action: rebind_state_transition_next_action(
				request.issue_identifier,
				existing_handoff.pr_url(),
			),
		});
	}

	(request.issue_state_name != request.success_state).then(|| HandoffBindingDiagnostic {
		classification: String::from(REVIEW_HANDOFF_MISMATCH_CLASSIFICATION),
		reason: String::from("review_handoff_issue_state_mismatch"),
		pr_base_ref,
		pr_head_oid,
		mismatched_field: Some(String::from("issue.state")),
		next_action: issue_state_mismatch_next_action(
			request.success_state,
			request.in_progress_state,
		),
	})
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
		"Run `decodex recover review-handoff rebind {issue_identifier} --pr {pr_url} --dry-run`, then rerun without `--dry-run` to refresh the retained lifecycle record if validation passes."
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
			"- issue: {}\n  state: {}\n  classification: {}\n  reason: {}\n  branch: {}\n  worktree_path: {}\n  local_branch: {}\n  local_head: {}\n  worktree_clean: {}\n  existing_pr_url: {}\n  existing_lifecycle_handoff_head: {}\n  existing_lifecycle_phase_head: {}\n  pr_base_ref: {}\n  pr_head: {}\n  mismatched_field: {}\n  active_label_present: {}\n  next_action: {}\n",
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
			optional_text(diagnostic.existing_lifecycle_handoff_head_oid.as_deref()),
			optional_text(diagnostic.existing_lifecycle_phase_head_oid.as_deref()),
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

fn diagnose_ghost_lanes<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	diagnose_ghost_lanes_with_listing_mode(
		project_id,
		worktree_root,
		state_store,
		tracker,
		selector,
		RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
	)
}

fn diagnose_ghost_lanes_read_only<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	diagnose_ghost_lanes_with_listing_mode(
		project_id,
		worktree_root,
		state_store,
		tracker,
		selector,
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)
}

fn diagnose_ghost_lanes_with_listing_mode<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<GhostLaneDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	let (mut runs, _) = if listing_mode.allows_runtime_writes() {
		state_store.list_project_runs(project_id, 0)?
	} else {
		state_store.list_project_runs_read_only(project_id, 0)?
	};

	if let Some(selector) = selector {
		let selector = selector.trim();

		runs.retain(|run| ghost_lane_run_matches_selector(run, selector));

		if runs.is_empty() {
			eyre::bail!("No leased lane matched `{selector}`.");
		}
		if runs.len() > 1 {
			eyre::bail!(
				"`{selector}` matched multiple leased lanes; pass the exact local issue id."
			);
		}
	}

	runs.into_iter()
		.map(|run| {
			inspect_ghost_lane(project_id, worktree_root, state_store, tracker, &run, selector)
		})
		.collect()
}

fn inspect_ghost_lane<T>(
	project_id: &str,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	run: &ProjectRunStatus,
	requested_selector: Option<&str>,
) -> Result<GhostLaneDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let issue_identifier = ghost_lane_issue_identifier(run, requested_selector);
	let mcp_test_fixture = ghost_lane_mcp_test_fixture_control_evidence(
		project_id,
		state_store,
		run,
		issue_identifier.as_deref(),
	)?;
	let mut evidence = Vec::new();
	let mut blockers = Vec::new();

	if run.run_lease() {
		evidence.push(String::from("run_lease_present"));
	} else {
		blockers.push(String::from("run_lease_missing"));
	}

	inspect_ghost_lane_tracker_issue(
		tracker,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_ghost_lane_worktree(
		worktree_root,
		state_store,
		run,
		issue_identifier.as_deref(),
		requested_selector,
		&mut evidence,
		&mut blockers,
	)?;

	let control_channel =
		inspect_ghost_lane_control_channel(run, mcp_test_fixture, &mut evidence, &mut blockers);

	inspect_ghost_lane_live_evidence(run, mcp_test_fixture, &mut evidence, &mut blockers);
	inspect_ghost_lane_private_evidence(
		project_id,
		state_store,
		run,
		mcp_test_fixture,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_ghost_lane_review_lineage(
		project_id,
		state_store,
		run,
		issue_identifier.as_deref(),
		&mut evidence,
		&mut blockers,
	)?;

	let (classification, reason, next_action) = if blockers.is_empty() {
		let (classification, reason) = if mcp_test_fixture {
			(
				String::from(MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_only_mcp_test_control_fixture_evidence"),
			)
		} else {
			(
				String::from(GHOST_LANE_CLASSIFICATION),
				String::from("tracker_issue_missing_and_no_live_or_retained_lane_evidence"),
			)
		};

		(
			classification,
			reason,
			format!(
				"Run `decodex recover ghost-lane cleanup {} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				issue_identifier.as_deref().unwrap_or(run.issue_id())
			),
		)
	} else {
		(
			String::from(GHOST_LANE_BLOCKED_CLASSIFICATION),
			String::from("safety_check_blocked"),
			String::from(
				"Preserve attention and inspect the listed blockers before using a recovery command.",
			),
		)
	};

	Ok(GhostLaneDiagnostic {
		project_id: project_id.to_owned(),
		issue_id: run.issue_id().to_owned(),
		issue_identifier,
		run_id: run.run_id().to_owned(),
		attempt_number: run.attempt_number(),
		attempt_status: run.status().to_owned(),
		classification,
		reason,
		run_lease: run.run_lease(),
		control_channel,
		evidence: sorted_unique(evidence),
		blockers: sorted_unique(blockers),
		next_action,
	})
}

fn diagnose_stale_active_issues<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	selector: Option<&str>,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<StaleActiveDiagnostic>>
where
	T: IssueTracker + ?Sized,
{
	let issues = if let Some(selector) = selector {
		vec![lookup_stale_active_issue(tracker, selector)?]
	} else {
		tracker.list_issues_with_label(&tracker::automation_active_label(project_id))?
	};

	issues
		.into_iter()
		.map(|issue| {
			inspect_stale_active_issue(
				project_id,
				workflow,
				worktree_root,
				state_store,
				tracker,
				issue,
				listing_mode,
			)
		})
		.collect()
}

fn lookup_stale_active_issue<T>(tracker: &T, selector: &str) -> Result<TrackerIssue>
where
	T: IssueTracker + ?Sized,
{
	let selector = selector.trim();

	if selector.is_empty() {
		eyre::bail!("Issue selector must not be empty.");
	}

	if commit_message::looks_like_issue_identifier(selector) {
		return tracker
			.get_issue_by_identifier(selector)?
			.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."));
	}

	if let Some(issue) = tracker.refresh_issues(&[selector.to_owned()])?.pop() {
		return Ok(issue);
	}

	tracker
		.get_issue_by_identifier(selector)?
		.ok_or_else(|| eyre::eyre!("No tracker issue matched `{selector}`."))
}

fn stale_active_issue_keys(issue_id: &str, issue_identifier: &str) -> Vec<String> {
	let mut keys = vec![issue_id.to_owned()];

	if issue_identifier != issue_id {
		keys.push(issue_identifier.to_owned());
	}
	keys
}

fn stale_active_tracker_issue_keys(issue: &TrackerIssue) -> Vec<String> {
	stale_active_issue_keys(&issue.id, &issue.identifier)
}

fn stale_active_diagnostic_issue_keys(diagnostic: &StaleActiveDiagnostic) -> Vec<String> {
	stale_active_issue_keys(&diagnostic.issue_id, &diagnostic.issue_identifier)
}

struct StaleActiveLabelSnapshot {
	queue_label_present: bool,
	active_label_present: bool,
	needs_attention_label_present: bool,
}

fn inspect_stale_active_labels<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	tracker: &T,
	issue: &TrackerIssue,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<StaleActiveLabelSnapshot>
where
	T: IssueTracker + ?Sized,
{
	let active_label = tracker::automation_active_label(project_id);
	let queue_label = tracker::automation_queue_label(project_id);
	let needs_attention_label = workflow.frontmatter().tracker().needs_attention_label();
	let active_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, &active_label)?;
	let queue_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, &queue_label)?;
	let needs_attention_label_present =
		tracker::issue_has_label_with_server_confirmation(tracker, issue, needs_attention_label)?;

	if active_label_present {
		evidence.push(String::from("active_label_present"));
	} else {
		blockers.push(String::from("active_label_missing"));
	}
	if queue_label_present {
		evidence.push(String::from("queue_label_present"));
	} else {
		evidence.push(String::from("queue_label_missing"));
	}
	if needs_attention_label_present {
		blockers.push(String::from("needs_attention_label_present"));
	} else {
		evidence.push(String::from("needs_attention_label_missing"));
	}

	Ok(StaleActiveLabelSnapshot {
		queue_label_present,
		active_label_present,
		needs_attention_label_present,
	})
}

fn inspect_stale_active_shared_claim(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> bool {
	let active_shared_claim =
		match stale_active_issue_has_active_shared_claim(project_id, state_store, issue_keys) {
			Ok(active_shared_claim) => active_shared_claim,
			Err(error) => {
				blockers.push(String::from("active_shared_claim_unknown"));
				evidence.push(format!("active_shared_claim_error:{}", error));

				false
			},
		};
	if active_shared_claim {
		blockers.push(String::from("active_shared_claim_present"));
	} else if !blockers.iter().any(|blocker| blocker == "active_shared_claim_unknown") {
		evidence.push(String::from("active_shared_claim_missing"));
	}

	active_shared_claim
}

fn inspect_stale_active_issue<T>(
	project_id: &str,
	workflow: &WorkflowDocument,
	worktree_root: &Path,
	state_store: &StateStore,
	tracker: &T,
	issue: TrackerIssue,
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut evidence = vec![String::from("tracker_issue_present")];
	let mut blockers = Vec::new();
	let issue_keys = stale_active_tracker_issue_keys(&issue);
	let labels = inspect_stale_active_labels(
		project_id,
		workflow,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;
	let active_shared_claim = inspect_stale_active_shared_claim(
		project_id,
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	);

	let runs = stale_active_runs(project_id, state_store, &issue_keys, listing_mode)?;
	let latest_run = latest_stale_active_run(&runs);
	let run_lease = runs.iter().any(ProjectRunStatus::run_lease);
	if run_lease {
		blockers.push(String::from("run_lease_present"));
	} else {
		evidence.push(String::from("run_lease_missing"));
	}
	let mapping = match stale_active_worktree_mapping_for_keys(state_store, &issue_keys) {
		Ok(mapping) => mapping,
		Err(error) => {
			blockers.push(String::from("worktree_mapping_ambiguous"));
			evidence.push(format!("worktree_mapping_error:{}", error));

			None
		},
	};
	let worktree_path = mapping
		.as_ref()
		.map(|mapping| mapping.worktree_path().to_path_buf())
		.unwrap_or_else(|| worktree_root.join(&issue.identifier));
	let marker = match state::read_run_activity_marker_snapshot(&worktree_path) {
		Ok(marker) => marker,
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			None
		},
	};
	let marker_liveness = stale_active_optional_marker_process_liveness(marker.as_ref());
	inspect_stale_active_run_evidence(&runs, marker_liveness, &mut evidence, &mut blockers);
	let worktree_state = inspect_stale_active_worktree(
		&worktree_path,
		mapping.as_ref(),
		marker.as_ref(),
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);
	let control_channel = inspect_stale_active_control_channel(
		latest_run,
		&runs,
		marker_liveness,
		&mut evidence,
		&mut blockers,
	);

	inspect_stale_active_private_evidence(
		project_id,
		state_store,
		&issue_keys,
		&mut evidence,
		&mut blockers,
	)?;
	inspect_stale_active_review_lineage(
		project_id,
		state_store,
		tracker,
		&issue,
		&mut evidence,
		&mut blockers,
	)?;

	let (classification, reason, next_action) = if blockers.is_empty() {
		(
			String::from(STALE_ACTIVE_CLASSIFICATION),
			String::from("tracker_issue_has_stale_active_label_without_live_or_retained_progress"),
			format!(
				"Run `decodex recover stale-active release {} --dry-run`, then rerun without `--dry-run` if the report stays safe.",
				issue.identifier
			),
		)
	} else {
		(
			String::from(STALE_ACTIVE_BLOCKED_CLASSIFICATION),
			String::from("safety_check_blocked"),
			String::from(
				"Preserve the lane and inspect the listed blockers before using a recovery command.",
			),
		)
	};

	Ok(StaleActiveDiagnostic {
		project_id: project_id.to_owned(),
		issue_id: issue.id,
		issue_identifier: issue.identifier,
		issue_state: issue.state.name,
		classification,
		reason,
		queue_label_present: labels.queue_label_present,
		active_label_present: labels.active_label_present,
		needs_attention_label_present: labels.needs_attention_label_present,
		latest_run_id: latest_run.map(|run| run.run_id().to_owned()),
		latest_attempt_number: latest_run.map(ProjectRunStatus::attempt_number),
		latest_attempt_status: latest_run.map(|run| run.status().to_owned()),
		run_lease,
		active_shared_claim,
		control_channel,
		worktree_path: Some(worktree_path.to_string_lossy().to_string()),
		worktree_state,
		evidence: sorted_unique(evidence),
		blockers: sorted_unique(blockers),
		next_action,
	})
}

fn stale_active_runs(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	listing_mode: RecoveryRuntimeMutationPolicy,
) -> Result<Vec<ProjectRunStatus>> {
	let mut runs = if listing_mode.allows_runtime_writes() {
		let mut runs = Vec::new();
		let mut seen_run_ids = HashSet::new();

		for issue_key in issue_keys {
			for run in state_store.list_project_issue_runs(project_id, issue_key)? {
				if seen_run_ids.insert(run.run_id().to_owned()) {
					runs.push(run);
				}
			}
		}

		runs
	} else {
		let (leased_runs, recent_runs) =
			state_store.list_project_runs_read_only(project_id, usize::MAX)?;
		let issue_key_set = issue_keys.iter().map(String::as_str).collect::<HashSet<_>>();
		leased_runs
			.into_iter()
			.chain(recent_runs)
			.filter(|run| issue_key_set.contains(run.issue_id()))
			.collect()
	};

	runs.sort_by(|left, right| {
		left.attempt_number()
			.cmp(&right.attempt_number())
			.then_with(|| left.run_id().cmp(right.run_id()))
	});

	Ok(runs)
}

fn stale_active_issue_has_active_shared_claim(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
) -> Result<bool> {
	for issue_key in issue_keys {
		if state_store.issue_has_active_shared_claim_read_only(project_id, issue_key)? {
			return Ok(true);
		}
	}

	Ok(false)
}

fn stale_active_worktree_mapping_for_keys(
	state_store: &StateStore,
	issue_keys: &[String],
) -> Result<Option<WorktreeMapping>> {
	let mut mapping = None;

	for issue_key in issue_keys {
		let Some(candidate) = state_store.worktree_for_issue(issue_key)? else {
			continue;
		};
		if let Some(existing) = mapping.as_ref() {
			if stale_active_worktree_mappings_conflict(existing, &candidate) {
				eyre::bail!(
					"conflicting retained worktree mappings for stale active issue keys `{}`",
					issue_keys.join(", ")
				);
			}
		} else {
			mapping = Some(candidate);
		}
	}

	Ok(mapping)
}

fn stale_active_worktree_mappings_conflict(
	left: &WorktreeMapping,
	right: &WorktreeMapping,
) -> bool {
	left.branch_name() != right.branch_name() || left.worktree_path() != right.worktree_path()
}

fn latest_stale_active_run(runs: &[ProjectRunStatus]) -> Option<&ProjectRunStatus> {
	runs.iter().max_by(|left, right| {
		left.attempt_number()
			.cmp(&right.attempt_number())
			.then_with(|| left.run_id().cmp(right.run_id()))
	})
}

fn inspect_stale_active_run_evidence(
	runs: &[ProjectRunStatus],
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if runs.is_empty() {
		evidence.push(String::from("run_attempt_missing"));
		evidence.push(String::from("protocol_event_evidence_missing"));
		evidence.push(String::from("child_agent_activity_missing"));
		evidence.push(String::from("protocol_activity_missing"));
		evidence.push(String::from("thread_reference_missing"));

		return;
	}

	evidence.push(String::from("run_attempt_present"));
	if runs.iter().any(|run| {
		run.event_count() > 0 || run.last_event_type().is_some() || run.last_event_at().is_some()
	}) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_protocol_event_evidence_present"));
		} else {
			blockers.push(String::from("protocol_event_evidence_present"));
		}
	} else {
		evidence.push(String::from("protocol_event_evidence_missing"));
	}
	if runs.iter().any(|run| run.child_agent_activity().is_some()) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_child_agent_activity_present"));
		} else {
			blockers.push(String::from("child_agent_activity_present"));
		}
	} else {
		evidence.push(String::from("child_agent_activity_missing"));
	}
	if runs.iter().any(|run| run.protocol_activity().is_some()) {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_protocol_activity_present"));
		} else {
			blockers.push(String::from("protocol_activity_present"));
		}
	} else {
		evidence.push(String::from("protocol_activity_missing"));
	}
	if runs.iter().any(|run| run.thread_id().is_some() || run.turn_id().is_some()) {
		evidence.push(String::from("stale_thread_reference_present"));
	} else {
		evidence.push(String::from("thread_reference_missing"));
	}
}

fn inspect_stale_active_worktree(
	worktree_path: &Path,
	mapping: Option<&WorktreeMapping>,
	marker: Option<&state::RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	if mapping.is_none() {
		evidence.push(String::from("worktree_mapping_missing"));
	}
	match worktree_path.try_exists() {
		Ok(false) => {
			evidence.push(String::from("worktree_missing"));

			return String::from("missing");
		},
		Ok(true) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}
	inspect_stale_active_activity_marker(marker, marker_liveness, evidence, blockers);
	match worktree_path.join(".git").try_exists() {
		Ok(false) => {
			match state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path) {
				Ok(true) => {
					evidence.push(String::from("worktree_non_git_marker_directory"));

					return String::from("non_git_marker_directory");
				},
				Ok(false) => {
					blockers.push(String::from("non_git_worktree_files_present"));

					return String::from("non_git_files_present");
				},
				Err(error) => {
					blockers.push(String::from("worktree_tracked_changes_unknown"));
					evidence.push(format!("worktree_status_error:{}", error));

					return String::from("tracked_changes_unknown");
				},
			}
		},
		Ok(true) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}
	match worktree_has_tracked_changes_for_recovery(worktree_path) {
		Ok(true) => {
			blockers.push(String::from("worktree_tracked_changes_present"));

			return String::from("tracked_changes_present");
		},
		Ok(false) => {},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}
	evidence.push(String::from("worktree_clean"));
	match worktree_head_has_unmerged_commits_against_remote_default(worktree_path) {
		Ok(Some(true)) => {
			blockers.push(String::from("worktree_unmerged_commits_present"));

			return String::from("unmerged_commits_present");
		},
		Ok(Some(false)) => {
			evidence.push(String::from("worktree_head_reachable_from_default_branch"));
		},
		Ok(None) => {
			blockers.push(String::from("worktree_default_branch_unavailable"));
			evidence.push(String::from("worktree_default_branch_unavailable"));

			return String::from("default_branch_unavailable");
		},
		Err(error) => {
			blockers.push(String::from("worktree_tracked_changes_unknown"));
			evidence.push(format!("worktree_head_status_error:{}", error));

			return String::from("tracked_changes_unknown");
		},
	}

	String::from("clean")
}

fn inspect_stale_active_activity_marker(
	marker: Option<&state::RunActivityMarker>,
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	if let Some(marker) = marker {
		match marker_liveness {
			StaleActiveProcessLiveness::Alive => blockers.push(String::from("process_alive")),
			StaleActiveProcessLiveness::NotAlive => {
				evidence.push(String::from("process_not_alive"))
			},
			StaleActiveProcessLiveness::Unknown => {
				blockers.push(String::from("process_liveness_unknown"))
			},
		}
		if marker.last_progress_unix_epoch().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_progress_present"));
			} else {
				blockers.push(String::from("activity_marker_progress_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_progress_missing"));
		}
		if marker.event_count() > 0 || marker.last_event_type().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_protocol_event_marker_present"));
			} else {
				blockers.push(String::from("protocol_event_marker_present"));
			}
		} else {
			evidence.push(String::from("protocol_event_marker_missing"));
		}
		if marker.last_protocol_activity_unix_epoch().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_protocol_activity_present"));
			} else {
				blockers.push(String::from("activity_marker_protocol_activity_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_protocol_activity_missing"));
		}
		if marker.child_agent_activity().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_child_agent_activity_present"));
			} else {
				blockers.push(String::from("activity_marker_child_agent_activity_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_child_agent_activity_missing"));
		}
		if marker.protocol_activity().is_some() {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence
					.push(String::from("stale_activity_marker_protocol_activity_summary_present"));
			} else {
				blockers.push(String::from("activity_marker_protocol_activity_summary_present"));
			}
		} else {
			evidence.push(String::from("activity_marker_protocol_activity_summary_missing"));
		}
		if stale_active_marker_thread_active(marker) {
			if marker_liveness == StaleActiveProcessLiveness::NotAlive {
				evidence.push(String::from("stale_activity_marker_thread_active"));
			} else {
				blockers.push(String::from("activity_marker_thread_active"));
			}
		} else {
			evidence.push(String::from("activity_marker_thread_inactive"));
		}
	} else {
		evidence.push(String::from("activity_marker_missing"));
	}
}

fn inspect_stale_active_control_channel(
	run: Option<&ProjectRunStatus>,
	runs: &[ProjectRunStatus],
	marker_liveness: StaleActiveProcessLiveness,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	let mut active_channel_present = false;

	for run in runs {
		let Some(channel) = run.control_channel() else {
			continue;
		};
		if channel.status() != RUN_CONTROL_CHANNEL_STATUS_ACTIVE {
			continue;
		}
		match channel.channel_path().try_exists() {
			Ok(true) => active_channel_present = true,
			Ok(false) => {},
			Err(error) => {
				blockers.push(String::from("active_control_channel_unknown"));
				evidence.push(format!("control_channel_status_error:{}", error));
			},
		}
	}

	if active_channel_present {
		if marker_liveness == StaleActiveProcessLiveness::NotAlive {
			evidence.push(String::from("stale_active_control_channel_present"));
		} else {
			blockers.push(String::from("active_control_channel_present"));
		}
	}

	let Some(channel) = run.and_then(ProjectRunStatus::control_channel) else {
		if !active_channel_present {
			evidence.push(String::from("control_channel_missing"));
		}

		return String::from("missing");
	};

	if !active_channel_present {
		evidence.push(String::from("control_channel_inactive_or_file_missing"));
	}

	format!("{}:{}", channel.transport(), channel.status())
}

fn inspect_stale_active_private_evidence(
	project_id: &str,
	state_store: &StateStore,
	issue_keys: &[String],
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut events = Vec::new();

	for issue_key in issue_keys {
		events.extend(state_store.list_private_execution_events_for_issue(project_id, issue_key)?);
	}

	if events.is_empty() {
		evidence.push(String::from("private_evidence_missing"));
	} else if events.iter().all(stale_active_private_event_allows_release) {
		evidence.push(String::from("only_stale_active_or_failed_control_evidence_present"));
	} else {
		blockers.push(String::from("private_progress_evidence_present"));
	}

	Ok(())
}

fn stale_active_private_event_allows_release(event: &PrivateExecutionEvent) -> bool {
	stale_active_private_event_is_release_audit(event)
		|| stale_active_private_event_is_failed_control_attempt(event)
		|| stale_active_private_event_is_stale_runtime_marker(event)
		|| stale_active_private_event_is_probing_checkpoint(event)
}

fn stale_active_private_event_is_release_audit(event: &PrivateExecutionEvent) -> bool {
	event.event_type() == STALE_ACTIVE_RELEASE_EVENT
		&& event.payload().get("schema").and_then(serde_json::Value::as_str)
			== Some(STALE_ACTIVE_RECOVERY_SCHEMA)
		&& event.payload().get("event").and_then(serde_json::Value::as_str)
			== Some(STALE_ACTIVE_RELEASE_EVENT)
}

fn stale_active_private_event_is_failed_control_attempt(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() == "lane_control/interrupt" {
		return event.payload().get("processAliveAfter").and_then(serde_json::Value::as_bool)
			== Some(false)
			&& event.payload().get("status").and_then(serde_json::Value::as_str) == Some("sent");
	}

	event.event_type() == "control_action"
		&& matches!(
			event.payload().get("action").and_then(serde_json::Value::as_str),
			Some("interrupt" | "steer")
		) && matches!(
		event.payload().get("reason").and_then(serde_json::Value::as_str),
		Some(
			"run_lease_missing"
				| "hard_fallback_unavailable"
				| "hard_interrupt_fallback"
				| "process_not_signalable"
		)
	)
}

fn stale_active_private_event_is_stale_runtime_marker(event: &PrivateExecutionEvent) -> bool {
	matches!(event.event_type(), "control_channel_published" | "phase_goal_set")
}

fn stale_active_private_event_is_probing_checkpoint(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != "progress_checkpoint" {
		return false;
	}
	let payload = event.payload();

	payload.get("phase").and_then(serde_json::Value::as_str) == Some("probing")
		&& json_string_is_missing_or_empty(payload.get("pr_url"))
		&& json_array_is_missing_or_empty(payload.get("verification"))
}

fn json_string_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_str().is_none_or(|value| value.is_empty()) || value.is_null())
}

fn json_array_is_missing_or_empty(value: Option<&serde_json::Value>) -> bool {
	value.is_none_or(|value| value.as_array().is_none_or(Vec::is_empty))
}

fn inspect_stale_active_review_lineage<T>(
	project_id: &str,
	state_store: &StateStore,
	tracker: &T,
	issue: &TrackerIssue,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	if state_store.issue_has_review_lifecycle_record(project_id, &issue.id)?
		|| (issue.identifier != issue.id
			&& state_store.issue_has_review_lifecycle_record(project_id, &issue.identifier)?)
	{
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if state_store.issue_has_review_policy_checkpoint(project_id, &issue.id)?
		|| (issue.identifier != issue.id
			&& state_store.issue_has_review_policy_checkpoint(project_id, &issue.identifier)?)
	{
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let records = stale_active_review_lineage_records(
		project_id,
		state_store,
		tracker,
		&issue.id,
		&issue.identifier,
	)?;

	if records.iter().any(ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		evidence.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn inspect_ghost_lane_tracker_issue<T>(
	tracker: &T,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let refreshed = match tracker.refresh_issues(&[run.issue_id().to_owned()]) {
		Ok(refreshed) => refreshed,
		Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, run.issue_id()) =>
			Vec::new(),
		Err(error) => return Err(error),
	};

	if !refreshed.is_empty() {
		blockers.push(String::from("tracker_issue_present"));

		return Ok(());
	}

	for selector in ghost_lane_tracker_issue_selectors(run, issue_identifier, requested_selector) {
		match tracker.get_issue_by_identifier(&selector) {
			Ok(Some(_)) => {
				blockers.push(String::from("tracker_issue_present"));

				return Ok(());
			},
			Ok(None) => {},
			Err(error) if tracker::issue_lookup_missing_error_for_candidate(&error, &selector) => {
			},
			Err(error) => return Err(error),
		}
	}

	evidence.push(String::from("tracker_issue_missing"));

	Ok(())
}

fn inspect_ghost_lane_worktree(
	worktree_root: &Path,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let mut retained_worktree_present = false;
	let mut mapping_checked = false;

	if let Some(worktree_path) = run.worktree_path() {
		mapping_checked = true;

		if worktree_path.exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}
	if let Some(mapping) = state_store.worktree_for_issue(run.issue_id())? {
		mapping_checked = true;

		if mapping.worktree_path().exists() {
			retained_worktree_present = true;
		} else {
			evidence.push(String::from("worktree_mapping_path_missing"));
		}
	}

	for selector in ghost_lane_worktree_selectors(run, issue_identifier, requested_selector) {
		if worktree_root.join(&selector).exists() {
			retained_worktree_present = true;
		}
	}

	if retained_worktree_present {
		blockers.push(String::from("retained_worktree_present"));
	} else {
		if !mapping_checked {
			evidence.push(String::from("worktree_mapping_missing"));
		}

		evidence.push(String::from("worktree_missing"));
	}

	Ok(())
}

fn inspect_ghost_lane_control_channel(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> String {
	let Some(channel) = run.control_channel() else {
		evidence.push(String::from("control_channel_missing"));

		return String::from("missing");
	};

	if channel.channel_path().exists() {
		evidence.push(String::from("control_channel_file_present"));
		blockers.push(String::from("control_channel_present"));
	} else {
		evidence.push(String::from("control_channel_file_missing"));

		if mcp_test_fixture {
			evidence.push(String::from("mcp_test_fixture_control_channel_row_present"));
		} else {
			blockers.push(String::from("control_channel_present"));
		}
	}

	format!("{}:present", channel.status())
}

fn inspect_ghost_lane_live_evidence(
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) {
	let mut live_blockers = Vec::new();

	if run.event_count() > 0 || run.last_event_type().is_some() || run.last_event_at().is_some() {
		live_blockers.push(String::from("protocol_event_evidence_present"));
	}
	if run.child_agent_activity().is_some() {
		live_blockers.push(String::from("child_agent_activity_present"));
	}
	if run.protocol_activity().is_some() {
		live_blockers.push(String::from("protocol_activity_present"));
	}
	if run.thread_id().is_some() || run.turn_id().is_some() {
		live_blockers.push(String::from("thread_reference_present"));
	}
	if live_blockers.is_empty() {
		evidence.push(String::from("no_live_execution_evidence"));

		return;
	}
	if mcp_test_fixture
		&& live_blockers
			.iter()
			.all(|blocker| ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker))
	{
		evidence.push(String::from("mcp_test_fixture_protocol_or_thread_evidence_present"));

		return;
	}

	blockers.extend(live_blockers);
}

fn inspect_ghost_lane_private_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	mcp_test_fixture: bool,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	if events.is_empty() {
		evidence.push(String::from("private_evidence_missing"));
	} else if mcp_test_fixture {
		evidence.push(String::from("mcp_test_fixture_private_control_evidence_present"));

		if events.iter().any(ghost_lane_private_event_is_cleanup_audit) {
			evidence.push(String::from("ghost_lane_cleanup_audit_present"));
		}
	} else if ghost_lane_private_events_are_cleanup_audit_evidence(&events) {
		evidence.push(String::from("ghost_lane_cleanup_audit_present"));
	} else {
		blockers.push(String::from("private_evidence_present"));
	}

	Ok(())
}

fn ghost_lane_mcp_test_fixture_control_evidence(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> Result<bool> {
	if !ghost_lane_has_mcp_test_fixture_identity(project_id, run, issue_identifier) {
		return Ok(false);
	}

	let events = state_store.list_private_execution_events(
		project_id,
		run.issue_id(),
		run.run_id(),
		run.attempt_number(),
	)?;

	Ok(ghost_lane_private_events_are_mcp_test_recovery_evidence(&events))
}

fn ghost_lane_has_mcp_test_fixture_identity(
	project_id: &str,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
) -> bool {
	project_id == MCP_TEST_FIXTURE_PROJECT_ID
		&& run.issue_id() == MCP_TEST_FIXTURE_ISSUE_ID
		&& run.run_id() == MCP_TEST_FIXTURE_RUN_ID
		&& run.attempt_number() == 1
		&& ghost_lane_mcp_test_fixture_issue_identifier_matches(issue_identifier)
		&& ghost_lane_optional_fixture_value(run.thread_id(), MCP_TEST_FIXTURE_THREAD_ID)
		&& ghost_lane_optional_fixture_value(run.turn_id(), MCP_TEST_FIXTURE_TURN_ID)
}

fn ghost_lane_mcp_test_fixture_issue_identifier_matches(issue_identifier: Option<&str>) -> bool {
	match issue_identifier {
		Some(value) =>
			value == MCP_TEST_FIXTURE_ISSUE_ID || value == MCP_TEST_FIXTURE_ALT_ISSUE_IDENTIFIER,
		None => true,
	}
}

fn ghost_lane_optional_fixture_value(value: Option<&str>, expected: &str) -> bool {
	match value {
		Some(value) => value == expected,
		None => true,
	}
}

fn ghost_lane_private_events_are_mcp_test_recovery_evidence(
	events: &[PrivateExecutionEvent],
) -> bool {
	!events.is_empty()
		&& events.iter().all(|event| {
			ghost_lane_private_event_is_mcp_test_control_evidence(event)
				|| ghost_lane_private_event_is_cleanup_audit(event)
		})
}

fn ghost_lane_private_event_is_mcp_test_control_evidence(event: &PrivateExecutionEvent) -> bool {
	match event.event_type() {
		"control_action" =>
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE)
				|| ghost_lane_cli_control_action_matches_mcp_test_fixture(event.payload()),
		"lane_control/steer/requested" | "lane_control/interrupt/requested" =>
			ghost_lane_private_event_source(event.payload()) == Some(MCP_TEST_FIXTURE_SOURCE),
		_ => false,
	}
}

fn ghost_lane_private_event_source(payload: &serde_json::Value) -> Option<&str> {
	payload
		.get("source")
		.and_then(serde_json::Value::as_str)
		.or_else(|| payload.pointer("/authority/source").and_then(serde_json::Value::as_str))
}

fn ghost_lane_cli_control_action_matches_mcp_test_fixture(payload: &serde_json::Value) -> bool {
	ghost_lane_private_event_source(payload) == Some("cli")
		&& matches!(
			payload.get("action").and_then(serde_json::Value::as_str),
			Some("steer" | "interrupt")
		) && payload.pointer("/requested/project_id").and_then(serde_json::Value::as_str)
		== Some(MCP_TEST_FIXTURE_PROJECT_ID)
		&& payload.pointer("/requested/issue_id").and_then(serde_json::Value::as_str)
			== Some(MCP_TEST_FIXTURE_ISSUE_ID)
		&& payload.pointer("/requested/run_id").and_then(serde_json::Value::as_str)
			== Some(MCP_TEST_FIXTURE_RUN_ID)
		&& payload.pointer("/requested/attempt_number").and_then(serde_json::Value::as_i64)
			== Some(1)
}

fn ghost_lane_private_events_are_cleanup_audit_evidence(events: &[PrivateExecutionEvent]) -> bool {
	!events.is_empty() && events.iter().all(ghost_lane_private_event_is_cleanup_audit)
}

fn ghost_lane_private_event_is_cleanup_audit(event: &PrivateExecutionEvent) -> bool {
	if event.event_type() != GHOST_LANE_CLEANUP_EVENT {
		return false;
	}

	let payload = event.payload();

	payload.get("schema").and_then(serde_json::Value::as_str)
		== Some("decodex.ghost_lane_recovery_private_event/1")
		&& payload.get("event").and_then(serde_json::Value::as_str)
			== Some(GHOST_LANE_CLEANUP_EVENT)
		&& matches!(
			payload.get("classification").and_then(serde_json::Value::as_str),
			Some(GHOST_LANE_CLASSIFICATION | MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION)
		) && payload.get("terminal_status").and_then(serde_json::Value::as_str)
		== Some(GHOST_LANE_TERMINAL_STATUS)
		&& payload.get("cleared_run_lease").and_then(serde_json::Value::as_bool) == Some(true)
		&& payload
			.get("blockers")
			.and_then(serde_json::Value::as_array)
			.is_some_and(|blockers| blockers.is_empty())
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "tracker_issue_missing")
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "worktree_missing")
		&& ghost_lane_cleanup_audit_evidence_contains(payload, "review_lineage_missing")
}

fn ghost_lane_cleanup_audit_evidence_contains(payload: &serde_json::Value, expected: &str) -> bool {
	payload
		.get("evidence")
		.and_then(serde_json::Value::as_array)
		.is_some_and(|evidence| evidence.iter().any(|entry| entry.as_str() == Some(expected)))
}

fn ghost_lane_mcp_test_fixture_allowed_live_blocker(blocker: &str) -> bool {
	matches!(
		blocker,
		"protocol_event_evidence_present"
			| "protocol_activity_present"
			| "thread_reference_present"
	)
}

fn inspect_ghost_lane_review_lineage(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	evidence: &mut Vec<String>,
	blockers: &mut Vec<String>,
) -> Result<()> {
	if state_store.issue_has_review_lifecycle_record(project_id, run.issue_id())? {
		blockers.push(String::from("review_lifecycle_present"));

		return Ok(());
	}
	if ghost_lane_run_has_review_policy_checkpoint(project_id, state_store, run)? {
		blockers.push(String::from("review_policy_checkpoint_present"));

		return Ok(());
	}

	let mut records = state_store.list_linear_execution_events(project_id, run.issue_id())?;

	if let Some(issue_identifier) = issue_identifier
		.filter(|issue_identifier| !issue_identifier.eq_ignore_ascii_case(run.issue_id()))
	{
		records.extend(state_store.list_linear_execution_events(project_id, issue_identifier)?);
	}

	if records.iter().any(ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push(String::from("pr_or_review_lineage_present"));
	} else {
		evidence.push(String::from("review_lineage_missing"));
	}

	Ok(())
}

fn ghost_lane_run_has_review_policy_checkpoint(
	project_id: &str,
	state_store: &StateStore,
	run: &ProjectRunStatus,
) -> Result<bool> {
	for phase in ["handoff", "repair"] {
		if state_store
			.review_policy_checkpoint(
				project_id,
				run.issue_id(),
				run.run_id(),
				run.attempt_number(),
				phase,
			)?
			.is_some()
		{
			return Ok(true);
		}
	}

	Ok(false)
}

fn ghost_lane_record_has_pr_or_review_lineage(record: &LinearExecutionEventRecord) -> bool {
	record.pr_url.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_head_sha.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| record.pr_base_ref.as_ref().is_some_and(|value| !value.trim().is_empty())
		|| matches!(
			record.event_type.as_str(),
			"review_handoff"
				| "review_handoff_rebind"
				| "review_handoff_adopt"
				| "review_repair"
				| "landed" | "closeout"
				| "cleanup_complete"
		) || record.terminal_path.as_deref() == Some("review_handoff")
}

fn apply_ghost_lane_cleanup(
	state_store: &StateStore,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			&diagnostic.project_id,
			&diagnostic.issue_id,
			&diagnostic.run_id,
			diagnostic.attempt_number,
			GHOST_LANE_CLEANUP_EVENT,
			serde_json::json!({
				"schema": "decodex.ghost_lane_recovery_private_event/1",
				"event": GHOST_LANE_CLEANUP_EVENT,
				"classification": &diagnostic.classification,
				"reason": &diagnostic.reason,
				"issue_identifier": &diagnostic.issue_identifier,
				"terminal_status": GHOST_LANE_TERMINAL_STATUS,
				"cleared_run_lease": true,
				"evidence": &diagnostic.evidence,
				"blockers": &diagnostic.blockers,
				"next_action": "ordinary automation may continue after status readback confirms no current attention lane",
			}),
		)
		.map(|_| ())?;
	state_store.update_run_status(&diagnostic.run_id, GHOST_LANE_TERMINAL_STATUS)?;
	state_store.retire_run_control_channel_for_attempt(
		&diagnostic.run_id,
		diagnostic.attempt_number,
		RUN_CONTROL_CHANNEL_STATUS_FAILED,
	)?;

	if let Some(mapping) = state_store.worktree_for_issue(&diagnostic.issue_id)?
		&& !mapping.worktree_path().exists()
	{
		state_store.clear_worktree(&diagnostic.issue_id)?;
	}

	state_store.clear_lease(&diagnostic.issue_id)
}

fn apply_stale_active_release(
	context: &RecoveryContext,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	apply_stale_active_release_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
}

fn apply_stale_active_release_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let diagnostic = refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		diagnostic,
	)?;
	let worktree_cleanup = preflight_stale_active_worktree_cleanup(state_store, &diagnostic)?;
	ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;
	ensure_stale_active_review_authority_missing(tracker, state_store, &diagnostic)?;
	let active_label = tracker::automation_active_label(config.service_id());

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		if diagnostic
			.latest_attempt_status
			.as_deref()
			.is_some_and(stale_active_attempt_status_needs_terminal_guard)
		{
			state_store.update_run_status(run_id, GHOST_LANE_TERMINAL_STATUS)?;
		}
		state_store.retire_run_control_channel_for_attempt(
			run_id,
			attempt_number,
			RUN_CONTROL_CHANNEL_STATUS_FAILED,
		)?;
	}

	ensure_stale_active_review_authority_missing(tracker, state_store, &diagnostic)?;
	cleanup_stale_active_worktree_mapping(
		config,
		workflow,
		state_store,
		&diagnostic,
		worktree_cleanup,
	)?;

	if let Some(run_id) = diagnostic.latest_run_id.as_deref()
		&& let Some(attempt_number) = diagnostic.latest_attempt_number
	{
		state_store
			.append_private_execution_event(
				&diagnostic.project_id,
				&diagnostic.issue_id,
				run_id,
				attempt_number,
				STALE_ACTIVE_RELEASE_EVENT,
				serde_json::json!({
					"schema": STALE_ACTIVE_RECOVERY_SCHEMA,
					"event": STALE_ACTIVE_RELEASE_EVENT,
					"phase": "local_cleanup_complete_before_active_label_release",
					"classification": &diagnostic.classification,
					"reason": &diagnostic.reason,
					"issue_identifier": &diagnostic.issue_identifier,
					"terminal_status": GHOST_LANE_TERMINAL_STATUS,
					"active_label_release": "pending_final_mutation",
					"queue_label_preserved": diagnostic.queue_label_present,
					"cleared_run_lease": false,
					"worktree_state": &diagnostic.worktree_state,
					"evidence": &diagnostic.evidence,
					"blockers": &diagnostic.blockers,
					"next_action": "ordinary automation may continue after status readback confirms no current attention lane",
				}),
			)
			.map(|_| ())?;
	}

	ensure_stale_active_review_authority_missing(tracker, state_store, &diagnostic)?;
	ensure_stale_active_run_claim_guard(config, state_store, &diagnostic)?;
	let final_diagnostic = refreshed_stale_active_release_diagnostic(
		tracker,
		config,
		workflow,
		state_store,
		&diagnostic,
	)?;
	ensure_stale_active_review_authority_missing(tracker, state_store, &final_diagnostic)?;
	ensure_stale_active_run_claim_guard(config, state_store, &final_diagnostic)?;
	let issue = lookup_stale_active_issue(tracker, &diagnostic.issue_identifier)?;

	tracker::set_issue_label_presence(tracker, &issue, &active_label, false)?;

	Ok(())
}

fn refreshed_stale_active_release_diagnostic<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	original: &StaleActiveDiagnostic,
) -> Result<StaleActiveDiagnostic>
where
	T: IssueTracker + ?Sized,
{
	let mut diagnostics = diagnose_stale_active_issues(
		config.service_id(),
		workflow,
		config.worktree_root(),
		state_store,
		tracker,
		Some(&original.issue_identifier),
		RecoveryRuntimeMutationPolicy::ReadOnly,
	)?;
	let diagnostic = diagnostics.pop().ok_or_else(|| {
		eyre::eyre!("No stale active issue matched `{}`.", original.issue_identifier)
	})?;

	if !diagnostic.recoverable() {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because safety inspection changed before apply: {}",
			original.issue_identifier,
			diagnostic.blockers.join(", ")
		);
	}
	if diagnostic.issue_id != original.issue_id
		|| diagnostic.latest_run_id != original.latest_run_id
		|| diagnostic.latest_attempt_number != original.latest_attempt_number
	{
		eyre::bail!(
			"`recover stale-active release` refused `{}` because the stale ownership target changed before apply.",
			original.issue_identifier
		);
	}

	Ok(diagnostic)
}

fn stale_active_attempt_status_needs_terminal_guard(status: &str) -> bool {
	matches!(status, "starting" | "running" | "continuation_pending" | "stalled")
}

fn ensure_stale_active_run_claim_guard(
	config: &ServiceConfig,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()> {
	let issue_keys = stale_active_diagnostic_issue_keys(diagnostic);

	match stale_active_issue_has_active_shared_claim(config.service_id(), state_store, &issue_keys)
	{
		Ok(false) => Ok(()),
		Ok(true) => eyre::bail!(
			"`recover stale-active release` refused `{}` because a run lease or shared claim appeared before active-label release.",
			diagnostic.issue_identifier
		),
		Err(error) => eyre::bail!(
			"`recover stale-active release` refused `{}` because run lease/shared claim state could not be inspected before active-label release: {}",
			diagnostic.issue_identifier,
			error
		),
	}
}

fn ensure_stale_active_review_authority_missing<T>(
	tracker: &T,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<()>
where
	T: IssueTracker + ?Sized,
{
	let mut blockers = Vec::new();

	if state_store
		.issue_has_review_lifecycle_record(&diagnostic.project_id, &diagnostic.issue_id)?
		|| (diagnostic.issue_identifier != diagnostic.issue_id
			&& state_store.issue_has_review_lifecycle_record(
				&diagnostic.project_id,
				&diagnostic.issue_identifier,
			)?) {
		blockers.push("review_lifecycle_present");
	}
	if state_store
		.issue_has_review_policy_checkpoint(&diagnostic.project_id, &diagnostic.issue_id)?
		|| (diagnostic.issue_identifier != diagnostic.issue_id
			&& state_store.issue_has_review_policy_checkpoint(
				&diagnostic.project_id,
				&diagnostic.issue_identifier,
			)?) {
		blockers.push("review_policy_checkpoint_present");
	}

	let records = stale_active_review_lineage_records(
		&diagnostic.project_id,
		state_store,
		tracker,
		&diagnostic.issue_id,
		&diagnostic.issue_identifier,
	)?;
	if records.iter().any(ghost_lane_record_has_pr_or_review_lineage) {
		blockers.push("pr_or_review_lineage_present");
	}

	if blockers.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"`recover stale-active release` refused `{}` because review authority appeared before active-label release: {}",
		diagnostic.issue_identifier,
		blockers.join(", ")
	)
}

fn stale_active_review_lineage_records<T>(
	project_id: &str,
	state_store: &StateStore,
	tracker: &T,
	issue_id: &str,
	issue_identifier: &str,
) -> Result<Vec<LinearExecutionEventRecord>>
where
	T: IssueTracker + ?Sized,
{
	let mut records = state_store.list_linear_execution_events(project_id, issue_id)?;

	if issue_identifier != issue_id {
		records.extend(state_store.list_linear_execution_events(project_id, issue_identifier)?);
	}

	let comments = tracker.list_comments(issue_id)?;

	records.extend(comments.iter().filter_map(|comment| {
		records::parse_linear_execution_event_record(&comment.body).filter(|record| {
			record.service_id == project_id
				&& (record.issue_id == issue_id
					|| record.issue_id == issue_identifier
					|| record.issue_identifier == issue_identifier
					|| record.issue_identifier == issue_id)
		})
	}));

	Ok(records)
}

#[derive(Clone, Debug)]
enum StaleActiveWorktreeCleanup {
	None,
	UnmappedPath(PathBuf),
	Mapped(WorktreeMapping),
}

fn preflight_stale_active_worktree_cleanup(
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
) -> Result<StaleActiveWorktreeCleanup> {
	let issue_keys = stale_active_diagnostic_issue_keys(diagnostic);
	let Some(mapping) = stale_active_worktree_mapping_for_keys(state_store, &issue_keys)? else {
		if let Some(worktree_path) = diagnostic.worktree_path.as_deref().map(PathBuf::from)
			&& stale_active_worktree_path_exists_for_cleanup(
				&diagnostic.issue_identifier,
				&worktree_path,
			)? {
			ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, &worktree_path)?;

			return Ok(StaleActiveWorktreeCleanup::UnmappedPath(worktree_path));
		}

		return Ok(StaleActiveWorktreeCleanup::None);
	};

	if stale_active_worktree_path_exists_for_cleanup(
		&diagnostic.issue_identifier,
		mapping.worktree_path(),
	)? {
		ensure_stale_active_worktree_clean(&diagnostic.issue_identifier, mapping.worktree_path())?;

		return Ok(StaleActiveWorktreeCleanup::Mapped(mapping));
	}

	Ok(StaleActiveWorktreeCleanup::None)
}

fn stale_active_worktree_path_exists_for_cleanup(
	issue_identifier: &str,
	worktree_path: &Path,
) -> Result<bool> {
	worktree_path.try_exists().wrap_err_with(|| {
		format!(
			"`recover stale-active release` refused `{}` because retained worktree `{}` could not be inspected before cleanup.",
			issue_identifier,
			worktree_path.display()
		)
	})
}

fn ensure_stale_active_worktree_clean(issue_identifier: &str, worktree_path: &Path) -> Result<()> {
	if worktree_has_tracked_changes_for_recovery(worktree_path)? {
		eyre::bail!(
			"`recover stale-active release` refused `{}` because retained worktree changes appeared before cleanup.",
			issue_identifier
		);
	}

	Ok(())
}

fn cleanup_stale_active_worktree_mapping(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &StaleActiveDiagnostic,
	cleanup: StaleActiveWorktreeCleanup,
) -> Result<()> {
	match cleanup {
		StaleActiveWorktreeCleanup::None => {},
		StaleActiveWorktreeCleanup::UnmappedPath(worktree_path) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);
			worktree_manager.remove_worktree_path(&worktree_path)?;
		},
		StaleActiveWorktreeCleanup::Mapped(mapping) => {
			let worktree_manager = WorktreeManager::new(
				config.service_id(),
				config.repo_root(),
				config.worktree_root(),
			);
			worktree_manager.remove_worktree_path_with_hooks(
				&diagnostic.issue_identifier,
				mapping.branch_name(),
				mapping.worktree_path(),
				workflow.frontmatter().execution().workspace_hooks(),
			)?;
		},
	};

	state_store.clear_worktree_mapping(&diagnostic.issue_id)?;
	if diagnostic.issue_identifier != diagnostic.issue_id {
		state_store.clear_worktree_mapping(&diagnostic.issue_identifier)?;
	}

	Ok(())
}

fn ensure_ghost_lane_live_status_allows_cleanup(
	context: &RecoveryContext,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()> {
	ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostic,
	)
}

fn ensure_ghost_lane_live_status_allows_cleanup_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostic: &GhostLaneDiagnostic,
) -> Result<()>
where
	T: IssueTracker,
{
	let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
		tracker,
		config,
		workflow,
		state_store,
		&diagnostic.issue_id,
		&diagnostic.run_id,
	)?;

	if blockers.is_empty() {
		return Ok(());
	}

	eyre::bail!(
		"`recover ghost-lane cleanup` refused `{}` because live status reported blockers: {}",
		render_ghost_lane_issue(diagnostic),
		blockers.join(", ")
	)
}

fn apply_ghost_lane_live_status_blockers(
	context: &RecoveryContext,
	diagnostics: &mut [GhostLaneDiagnostic],
) -> Result<()> {
	apply_ghost_lane_live_status_blockers_with_tracker(
		&context.tracker,
		&context.config,
		&context.workflow,
		&context.state_store,
		diagnostics,
	)
}

fn apply_ghost_lane_live_status_blockers_with_tracker<T>(
	tracker: &T,
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	diagnostics: &mut [GhostLaneDiagnostic],
) -> Result<()>
where
	T: IssueTracker,
{
	for diagnostic in diagnostics {
		let blockers = orchestrator::ghost_lane_cleanup_status_blockers(
			tracker,
			config,
			workflow,
			state_store,
			&diagnostic.issue_id,
			&diagnostic.run_id,
		)?;

		if blockers.is_empty() {
			continue;
		}

		diagnostic.classification = String::from(GHOST_LANE_BLOCKED_CLASSIFICATION);
		diagnostic.reason = String::from("status_safety_check_blocked");
		diagnostic.next_action = String::from(
			"Preserve attention and inspect the listed blockers before using a recovery command.",
		);
		diagnostic.blockers = sorted_unique(
			diagnostic
				.blockers
				.iter()
				.cloned()
				.chain(blockers.into_iter().map(|blocker| format!("status:{blocker}")))
				.collect(),
		);
	}

	Ok(())
}

fn render_ghost_lane_recovery_report(report: &GhostLaneRecoveryReport) -> String {
	let mut output = format!("Ghost lane recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  local_issue_id: {}\n  run_id: {}\n  attempt: {}\n  attempt_status: {}\n  classification: {}\n  reason: {}\n  run_lease: {}\n  control_channel: {}\n  evidence: {}\n  blockers: {}\n  next_action: {}\n",
			render_ghost_lane_issue(diagnostic),
			diagnostic.issue_id,
			diagnostic.run_id,
			diagnostic.attempt_number,
			diagnostic.attempt_status,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.run_lease,
			diagnostic.control_channel,
			render_string_list(&diagnostic.evidence),
			render_string_list(&diagnostic.blockers),
			diagnostic.next_action,
		));
	}

	output
}

fn render_stale_active_recovery_report(report: &StaleActiveRecoveryReport) -> String {
	let mut output =
		format!("Stale active recovery diagnostics for project {}\n", report.project_id);

	if report.diagnostics.is_empty() {
		output.push_str("- none\n");

		return output;
	}

	for diagnostic in &report.diagnostics {
		output.push_str(&format!(
			"- issue: {}\n  issue_id: {}\n  issue_state: {}\n  classification: {}\n  reason: {}\n  queue_label_present: {}\n  active_label_present: {}\n  needs_attention_label_present: {}\n  latest_run_id: {}\n  latest_attempt: {}\n  latest_attempt_status: {}\n  run_lease: {}\n  active_shared_claim: {}\n  control_channel: {}\n  worktree_path: {}\n  worktree_state: {}\n  evidence: {}\n  blockers: {}\n  next_action: {}\n",
			diagnostic.issue_identifier,
			diagnostic.issue_id,
			diagnostic.issue_state,
			diagnostic.classification,
			diagnostic.reason,
			diagnostic.queue_label_present,
			diagnostic.active_label_present,
			diagnostic.needs_attention_label_present,
			diagnostic.latest_run_id.as_deref().unwrap_or("none"),
			diagnostic
				.latest_attempt_number
				.map(|attempt| attempt.to_string())
				.unwrap_or_else(|| String::from("none")),
			diagnostic.latest_attempt_status.as_deref().unwrap_or("none"),
			diagnostic.run_lease,
			diagnostic.active_shared_claim,
			diagnostic.control_channel,
			diagnostic.worktree_path.as_deref().unwrap_or("none"),
			diagnostic.worktree_state,
			render_string_list(&diagnostic.evidence),
			render_string_list(&diagnostic.blockers),
			diagnostic.next_action,
		));
	}

	output
}

fn render_ghost_lane_issue(diagnostic: &GhostLaneDiagnostic) -> &str {
	diagnostic.issue_identifier.as_deref().unwrap_or(diagnostic.issue_id.as_str())
}

fn render_string_list(values: &[String]) -> String {
	if values.is_empty() { String::from("none") } else { values.join(",") }
}

fn ghost_lane_issue_identifier(
	run: &ProjectRunStatus,
	requested_selector: Option<&str>,
) -> Option<String> {
	requested_selector
		.filter(|selector| commit_message::looks_like_issue_identifier(selector))
		.map(str::to_ascii_uppercase)
		.or_else(|| ghost_lane_issue_identifier_from_run_id(run.run_id()))
		.or_else(|| run.branch_name().and_then(ghost_lane_issue_identifier_in_text))
		.or_else(|| {
			run.worktree_path()
				.and_then(|path| ghost_lane_issue_identifier_in_text(&path.display().to_string()))
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(run.issue_id())
				.then(|| run.issue_id().to_ascii_uppercase())
		})
}

fn ghost_lane_inferred_issue_identifier(run: &ProjectRunStatus) -> Option<String> {
	ghost_lane_issue_identifier_from_run_id(run.run_id())
		.or_else(|| run.branch_name().and_then(ghost_lane_issue_identifier_in_text))
		.or_else(|| {
			run.worktree_path()
				.and_then(|path| ghost_lane_issue_identifier_in_text(&path.display().to_string()))
		})
		.or_else(|| {
			commit_message::looks_like_issue_identifier(run.issue_id())
				.then(|| run.issue_id().to_ascii_uppercase())
		})
}

fn ghost_lane_issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return ghost_lane_issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return ghost_lane_issue_identifier_in_text(candidate);
	}

	None
}

fn ghost_lane_issue_identifier_in_text(value: &str) -> Option<String> {
	let bytes = value.as_bytes();

	for index in 0..bytes.len() {
		if !bytes[index].is_ascii_alphabetic() {
			continue;
		}

		let mut prefix_end = index + 1;

		while prefix_end < bytes.len() && bytes[prefix_end].is_ascii_alphanumeric() {
			prefix_end += 1;
		}

		if prefix_end >= bytes.len() || bytes[prefix_end] != b'-' {
			continue;
		}

		let mut digit_end = prefix_end + 1;

		while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
			digit_end += 1;
		}

		if digit_end > prefix_end + 1 {
			return Some(value[index..digit_end].to_ascii_uppercase());
		}
	}

	None
}

fn ghost_lane_tracker_issue_selectors(
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
) -> Vec<String> {
	let mut selectors = Vec::new();

	if let Some(selector) =
		requested_selector.filter(|selector| commit_message::looks_like_issue_identifier(selector))
	{
		selectors.push(selector.to_ascii_uppercase());
	}
	if let Some(issue_identifier) = issue_identifier {
		selectors.push(issue_identifier.to_ascii_uppercase());
	}
	if let Some(inferred) = ghost_lane_inferred_issue_identifier(run) {
		selectors.push(inferred);
	}

	if commit_message::looks_like_issue_identifier(run.issue_id()) {
		selectors.push(run.issue_id().to_ascii_uppercase());
	}

	sorted_unique(selectors)
}

fn ghost_lane_worktree_selectors(
	run: &ProjectRunStatus,
	issue_identifier: Option<&str>,
	requested_selector: Option<&str>,
) -> Vec<String> {
	let mut selectors = Vec::new();

	if let Some(selector) =
		requested_selector.filter(|selector| commit_message::looks_like_issue_identifier(selector))
	{
		selectors.push(selector.to_ascii_uppercase());
	}
	if let Some(issue_identifier) = issue_identifier {
		selectors.push(issue_identifier.to_ascii_uppercase());
	}
	if let Some(inferred) = ghost_lane_inferred_issue_identifier(run) {
		selectors.push(inferred);
	}

	if commit_message::looks_like_issue_identifier(run.issue_id()) {
		selectors.push(run.issue_id().to_ascii_uppercase());
	}

	sorted_unique(selectors)
}

fn ghost_lane_run_matches_selector(run: &ProjectRunStatus, selector: &str) -> bool {
	if selector.eq_ignore_ascii_case(run.issue_id()) || selector.eq_ignore_ascii_case(run.run_id())
	{
		return true;
	}

	ghost_lane_worktree_selectors(run, ghost_lane_inferred_issue_identifier(run).as_deref(), None)
		.iter()
		.any(|candidate| {
			selector.eq_ignore_ascii_case(candidate)
				|| ghost_lane_identifier_suffix_matches(selector, candidate)
		})
}

fn ghost_lane_identifier_suffix_matches(left: &str, right: &str) -> bool {
	let Some((left_prefix, left_suffix)) = ghost_lane_identifier_parts(left) else {
		return false;
	};
	let Some((right_prefix, right_suffix)) = ghost_lane_identifier_parts(right) else {
		return false;
	};

	left_suffix == right_suffix
		&& (left_prefix.eq_ignore_ascii_case(right_prefix)
			|| left_prefix.to_ascii_uppercase().starts_with(&right_prefix.to_ascii_uppercase())
			|| right_prefix.to_ascii_uppercase().starts_with(&left_prefix.to_ascii_uppercase()))
}

fn ghost_lane_identifier_parts(value: &str) -> Option<(&str, &str)> {
	let (prefix, suffix) = value.rsplit_once('-')?;

	(!prefix.is_empty() && suffix.chars().all(|character| character.is_ascii_digit()))
		.then_some((prefix, suffix))
}

fn sorted_unique(values: Vec<String>) -> Vec<String> {
	let mut set = BTreeSet::new();

	for value in values {
		set.insert(value);
	}

	set.into_iter().collect()
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
		restore_active_label: label_validation.restore_active_label,
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
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}`; refusing to rebind it to `{}`.",
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
			"Issue `{}` already has a review lifecycle record for branch `{}` and PR `{}` at head `{local_head_oid}`; no rebind is needed.",
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
	let tracker_policy = context.workflow.frontmatter().tracker();

	if !active_label_present {
		if !mode.allows_failure_state_drift_repair()
			|| issue.state.name != tracker_policy.failure_state()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`. Restore explicit lane ownership before rebind.",
				issue.identifier
			);
		}

		if tracker::issue_team_label_id_with_server_confirmation(
			&context.tracker,
			issue,
			&active_label,
		)?
		.is_none()
		{
			eyre::bail!(
				"Issue `{}` is missing active automation label `{active_label}`, but that label was not found on the team.",
				issue.identifier
			);
		}
	}

	let needs_attention_label = tracker_policy.needs_attention_label();
	let needs_attention_present = tracker::issue_has_label_with_server_confirmation(
		&context.tracker,
		issue,
		needs_attention_label,
	)?;

	if !needs_attention_present {
		return Ok(RebindLabelValidation {
			active_label_present,
			restore_active_label: !active_label_present,
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
			restore_active_label: !active_label_present,
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

	Ok(RebindLabelValidation {
		active_label_present,
		restore_active_label: false,
		clear_needs_attention_label: false,
	})
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
	let decision = pull_request::classify_landing_gate(gate_view, LandingGateMode::Adopt);

	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(()),
		decision => adopt_landing_gate_error(decision, gate_view, pr_url),
	}
}

fn adopt_landing_gate_error(
	decision: pull_request::LandingGateDecision,
	gate_view: PullRequestLandingGateView<'_>,
	pr_url: &str,
) -> Result<()> {
	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(()),
		pull_request::LandingGateDecision::CloseoutOnly
		| pull_request::LandingGateDecision::Block("pull_request_not_open") => {
			eyre::bail!("Pull request `{pr_url}` is `{}`; adopt requires `OPEN`.", gate_view.state)
		},
		pull_request::LandingGateDecision::Block("pull_request_is_draft") => {
			eyre::bail!("Pull request `{pr_url}` is still draft.")
		},
		pull_request::LandingGateDecision::Wait("pending_review_requests") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} pending review request(s).",
				gate_view.pending_review_requests
			)
		},
		pull_request::LandingGateDecision::Repair("unresolved_review_threads") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} unresolved review thread(s).",
				gate_view.unresolved_review_threads
			)
		},
		pull_request::LandingGateDecision::Repair("review_changes_requested") => {
			eyre::bail!("Pull request `{pr_url}` still has active change requests.")
		},
		pull_request::LandingGateDecision::Repair(reason)
			if matches!(
				reason,
				"pull_request_merge_conflict" | "pull_request_branch_behind_base"
			) =>
		{
			eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.")
		},
		pull_request::LandingGateDecision::Repair("required_checks_failed") => {
			eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.")
		},
		pull_request::LandingGateDecision::Wait("checks_waiting") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait("mergeability_unknown") => {
			eyre::bail!("Pull request `{pr_url}` mergeability is still unknown.")
		},
		pull_request::LandingGateDecision::Block("merge_state_not_ready") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not ready to adopt: mergeStateStatus=`{}`.",
				gate_view.merge_state_status
			)
		},
		pull_request::LandingGateDecision::Block("not_mergeable") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
				gate_view.mergeable
			)
		},
		pull_request::LandingGateDecision::Wait("checks_non_green") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait(reason)
		| pull_request::LandingGateDecision::Repair(reason)
		| pull_request::LandingGateDecision::Block(reason) => {
			eyre::bail!("Pull request `{pr_url}` is not ready to adopt: {reason}.")
		},
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
				"Issue `{}` already has a retained review lifecycle record for branch `{branch}`; use `decodex land` or `decodex recover review-handoff rebind` instead.",
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

	write_review_lifecycle_markers_with_rollback(
		&context.state_store,
		context.config.service_id(),
		&validation.issue.id,
		&handoff_marker,
		&orchestration_marker,
		|| {
			context.state_store.upsert_review_orchestration_marker(
				context.config.service_id(),
				&validation.issue.id,
				&orchestration_marker,
			)
		},
	)?;

	let active_label_restored = match restore_rebind_active_label(context, validation) {
		Ok(active_label_restored) => active_label_restored,
		Err(error) => {
			context.state_store.clear_review_lifecycle_for_handoff(
				context.config.service_id(),
				&validation.issue.id,
				&handoff_marker,
				&orchestration_marker,
			)?;

			return Err(error);
		},
	};
	let event = review_handoff_rebind_event(context, validation, active_label_restored);

	if let Err(error) = write_rebind_audit(context, validation, &event)
		.and_then(|()| context.state_store.record_linear_execution_event(&event))
	{
		context.state_store.clear_review_lifecycle_for_handoff(
			context.config.service_id(),
			&validation.issue.id,
			&handoff_marker,
			&orchestration_marker,
		)?;

		rollback_rebind_active_label_restoration(context, validation, active_label_restored)?;

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

	append_review_handoff_rebind_private_event(
		&context.state_store,
		context.config.service_id(),
		validation,
		"local_markers_written",
		active_label_restored,
	)?;

	Ok(())
}

fn write_review_lifecycle_markers_with_rollback<F>(
	state_store: &StateStore,
	project_id: &str,
	issue_id: &str,
	handoff_marker: &ReviewHandoffMarker,
	orchestration_marker: &ReviewOrchestrationMarker,
	write_orchestration_marker: F,
) -> Result<()>
where
	F: FnOnce() -> Result<()>,
{
	if let Err(error) = state_store
		.upsert_review_handoff_marker(project_id, issue_id, handoff_marker)
		.and_then(|()| write_orchestration_marker())
	{
		state_store.clear_review_lifecycle_for_handoff(
			project_id,
			issue_id,
			handoff_marker,
			orchestration_marker,
		)?;

		return Err(error);
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
	let local_state_write =
		write_adopt_local_state(context, validation, &handoff_marker, &orchestration_marker);

	if let Err(error) = local_state_write {
		mark_adopt_attempt_failed(context, validation);

		context.state_store.clear_review_lifecycle_for_handoff(
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

			context.state_store.clear_review_lifecycle_for_handoff(
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

		context.state_store.clear_review_lifecycle_for_handoff(
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

	append_review_handoff_adopt_private_event(
		&context.state_store,
		context.config.service_id(),
		validation,
		"active_label_checked",
		active_label_restored,
	)?;

	Ok(())
}

fn write_adopt_local_state(
	context: &RecoveryContext,
	validation: &AdoptValidation,
	handoff_marker: &ReviewHandoffMarker,
	orchestration_marker: &ReviewOrchestrationMarker,
) -> Result<()> {
	let worktree_path = validation.worktree_path.to_string_lossy().to_string();

	context
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
				handoff_marker,
			)
		})
		.and_then(|()| {
			context.state_store.upsert_review_orchestration_marker(
				context.config.service_id(),
				&validation.issue.id,
				orchestration_marker,
			)
		})
}

fn restore_rebind_active_label(
	context: &RecoveryContext,
	validation: &RebindValidation,
) -> Result<bool> {
	if !validation.should_restore_active_label() {
		return Ok(false);
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, true)
}

fn rollback_rebind_active_label_restoration(
	context: &RecoveryContext,
	validation: &RebindValidation,
	active_label_restored: bool,
) -> Result<()> {
	if !active_label_restored {
		return Ok(());
	}

	let active_label = tracker::automation_active_label(context.config.service_id());

	tracker::set_issue_label_presence(&context.tracker, &validation.issue, &active_label, false)?;

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

fn append_review_handoff_rebind_private_event(
	state_store: &StateStore,
	service_id: &str,
	validation: &RebindValidation,
	writeback_stage: &str,
	active_label_restored: bool,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			service_id,
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
			REVIEW_HANDOFF_REBIND_EVENT,
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": REVIEW_HANDOFF_REBIND_EVENT,
				"writeback_stage": writeback_stage,
				"issue_identifier": &validation.issue.identifier,
				"branch": validation.worktree.branch_name(),
				"worktree_path": &validation.worktree_path_for_event,
				"pr_url": landing_url(&validation.landing_state),
				"pr_head_sha": &validation.local_head_oid,
				"pr_base_ref": &validation.landing_state.base_ref_name,
				"pr_state": &validation.landing_state.state,
				"mergeable": &validation.landing_state.mergeable,
				"merge_state_status": &validation.landing_state.merge_state_status,
				"status_check_rollup_state": &validation.landing_state.status_check_rollup_state,
				"mode": validation.mode.as_str(),
				"active_label_present": validation.active_label_present,
				"active_label_restored": active_label_restored,
				"clear_needs_attention_label": validation.clear_needs_attention_label,
				"next_action": "continue retained post-review lifecycle",
			}),
		)
		.map(|_| ())
}

fn append_review_handoff_adopt_private_event(
	state_store: &StateStore,
	service_id: &str,
	validation: &AdoptValidation,
	writeback_stage: &str,
	active_label_restored: bool,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			service_id,
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
			REVIEW_HANDOFF_ADOPT_EVENT,
			serde_json::json!({
				"schema": "decodex.review_handoff_recovery_private_event/1",
				"event": REVIEW_HANDOFF_ADOPT_EVENT,
				"writeback_stage": writeback_stage,
				"issue_identifier": &validation.issue.identifier,
				"branch": &validation.branch_name,
				"worktree_path": &validation.worktree_path_for_event,
				"pr_url": landing_url(&validation.landing_state),
				"pr_head_sha": &validation.local_head_oid,
				"pr_base_ref": &validation.landing_state.base_ref_name,
				"pr_state": &validation.landing_state.state,
				"mergeable": &validation.landing_state.mergeable,
				"merge_state_status": &validation.landing_state.merge_state_status,
				"status_check_rollup_state": &validation.landing_state.status_check_rollup_state,
				"active_label_present": validation.active_label_present,
				"active_label_restored": active_label_restored,
				"existing_retained_worktree_mapping": validation.previous_worktree_mapping.is_some(),
				"existing_review_handoff_marker": false,
				"manual_takeover_adopt": true,
				"next_action": "continue retained post-review lifecycle",
			}),
		)
		.map(|_| ())
}

fn review_handoff_rebind_event(
	context: &RecoveryContext,
	validation: &RebindValidation,
	active_label_restored: bool,
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
		format!("existing_review_lifecycle_record={}", validation.mode.evidence_value()),
		format!("active_label_present={}", validation.active_label_present),
		format!("active_label_repair={active_label_restored}"),
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
		String::from("existing_review_lifecycle_record=false"),
	]);
	event.next_action = Some(String::from("continue retained post-review lifecycle"));

	event
}

fn write_rebind_audit(
	context: &RecoveryContext,
	validation: &RebindValidation,
	event: &LinearExecutionEventRecord,
) -> Result<()> {
	let recovery_body = format!(
		"Decodex operator recovery: {} for `{}` to `{}`. This does not land the pull request.",
		validation.mode.summary_action(),
		validation.issue.identifier,
		landing_url(&validation.landing_state)
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{recovery_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
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
	let recovery_body = format!(
		"Decodex operator recovery: adopted human-owned PR `{}` for `{}` into retained review handoff state. This does not land the pull request.",
		landing_url(&validation.landing_state),
		validation.issue.identifier,
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{recovery_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
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
	let audit_body = format!(
		"Decodex legacy manual closeout audit: verified merged PR `{}` for `{}`. Runtime provenance was `{}`, so this records the manual fallback before local cleanup.",
		landing_url(&validation.landing_state),
		validation.issue.identifier,
		validation.worktree.provenance().source()
	);
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{audit_body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
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

	context.state_store.update_run_status(&validation.run_id, "succeeded")?;

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
	let retry_budget_attempt_count =
		context.state_store.retry_budget_attempt_count(&validation.issue.id)?;
	let retry_budget_attempt_count =
		(retry_budget_attempt_count > 0).then_some(retry_budget_attempt_count);
	let body = format!(
		"{body}\n\n{}",
		records::render_linear_execution_event_comment_body(event, retry_budget_attempt_count)
	);
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

fn worktree_has_tracked_changes_for_recovery(worktree_path: &Path) -> Result<bool> {
	if !worktree_path.try_exists()? {
		return Ok(false);
	}
	if !worktree_path.join(".git").try_exists()? {
		return Ok(!state::retained_path_contains_only_decodex_runtime_artifacts(worktree_path)?);
	}

	Ok(!worktree_blocking_status_lines(worktree_path)?.is_empty())
}

fn worktree_head_has_unmerged_commits_against_remote_default(
	worktree_path: &Path,
) -> Result<Option<bool>> {
	let Some(default_ref) = worktree_remote_default_ref(worktree_path)? else {
		return Ok(None);
	};
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["merge-base", "--is-ancestor", "HEAD", default_ref.as_str()])
		.output()?;

	match output.status.code() {
		Some(0) => Ok(Some(false)),
		Some(1) => Ok(Some(true)),
		status => {
			let stderr = String::from_utf8_lossy(&output.stderr);

			eyre::bail!(
				"Failed to compare retained worktree HEAD in `{}` against `{default_ref}`: status={:?} {}",
				worktree_path.display(),
				status,
				stderr.trim()
			)
		},
	}
}

fn worktree_remote_default_ref(worktree_path: &Path) -> Result<Option<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
		.output()?;
	if output.status.success() {
		let value = trimmed_stdout(&output.stdout)?;

		if !value.is_empty() {
			return Ok(Some(value));
		}
	}

	for candidate in ["origin/main", "main"] {
		let revision = format!("{candidate}^{{commit}}");
		let output = Command::new("git")
			.arg("-C")
			.arg(worktree_path)
			.args(["rev-parse", "--verify", "--quiet", revision.as_str()])
			.output()?;
		if output.status.success() {
			return Ok(Some(candidate.to_owned()));
		}
	}

	Ok(None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StaleActiveProcessLiveness {
	Alive,
	NotAlive,
	Unknown,
}

fn stale_active_optional_marker_process_liveness(
	marker: Option<&state::RunActivityMarker>,
) -> StaleActiveProcessLiveness {
	marker.map(stale_active_marker_process_liveness).unwrap_or(StaleActiveProcessLiveness::Unknown)
}

fn stale_active_marker_process_liveness(
	marker: &state::RunActivityMarker,
) -> StaleActiveProcessLiveness {
	let Some(process_id) = marker.process_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if !stale_active_process_is_alive(process_id) {
		return StaleActiveProcessLiveness::NotAlive;
	}
	let Some(marker_host_boot_id) = marker.host_boot_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};
	let Some(current_host_boot_id) = state::current_host_boot_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if marker_host_boot_id != current_host_boot_id.as_str() {
		return StaleActiveProcessLiveness::NotAlive;
	}

	let Some(marker_process_start_identity) = marker.process_start_identity() else {
		return StaleActiveProcessLiveness::Unknown;
	};
	let Some(current_process_start_identity) = state::process_start_identity(process_id) else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if marker_process_start_identity == current_process_start_identity.as_str() {
		StaleActiveProcessLiveness::Alive
	} else {
		StaleActiveProcessLiveness::NotAlive
	}
}

fn stale_active_marker_thread_active(marker: &state::RunActivityMarker) -> bool {
	matches!(marker.thread_status(), Some("active")) || !marker.thread_active_flags().is_empty()
}

#[cfg(unix)]
fn stale_active_process_is_alive(process_id: u32) -> bool {
	let Ok(process_id) = libc::pid_t::try_from(process_id) else {
		return false;
	};

	if process_id <= 0 {
		return false;
	}

	match unsafe { libc::kill(process_id, 0) } {
		0 => true,
		-1 => matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM)),
		_ => false,
	}
}

#[cfg(not(unix))]
fn stale_active_process_is_alive(_process_id: u32) -> bool {
	false
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
	use std::{cell::RefCell, fs, path::Path, process::Command};

	use tempfile::TempDir;

	use crate::{
		config::ServiceConfig,
		pull_request::PullRequestLandingState,
		recovery::{
			GHOST_LANE_BLOCKED_CLASSIFICATION, GHOST_LANE_CLASSIFICATION, GHOST_LANE_CLEANUP_EVENT,
			GHOST_LANE_TERMINAL_STATUS, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
			REVIEW_HANDOFF_ADOPT_EVENT, REVIEW_HANDOFF_BOUND_CLASSIFICATION,
			REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION, REVIEW_HANDOFF_REBIND_EVENT,
			REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION, STALE_ACTIVE_CLASSIFICATION,
			STALE_ACTIVE_RELEASE_EVENT,
		},
		state::{
			self, ChildAgentActivitySummary, ConnectorBackoffInput, ProtocolActivityMarker,
			ProtocolActivitySummary, ReviewHandoffMarker, ReviewOrchestrationMarker,
			ReviewPolicyCheckpointInput, StateStore, WorktreeMapping,
		},
		tracker::{
			self, IssueTracker, TrackerComment, TrackerIssue, TrackerLabel, TrackerState,
			TrackerTeam,
			linear::LinearClient,
			records::{self, LinearExecutionEventIdentity, LinearExecutionEventRecord},
		},
		workflow::WorkflowDocument,
	};

	struct GhostLaneTestTracker {
		issues: Vec<TrackerIssue>,
		refresh_error: Option<String>,
		identifier_error: Option<String>,
		remove_error: Option<String>,
		comments: Vec<TrackerComment>,
		refresh_queries: RefCell<Vec<Vec<String>>>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
	}
	impl GhostLaneTestTracker {
		fn missing() -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: None,
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn with_issues(issues: Vec<TrackerIssue>) -> Self {
			Self {
				issues,
				refresh_error: None,
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn with_comments(mut self, comments: Vec<TrackerComment>) -> Self {
			self.comments = comments;
			self
		}

		fn remove_error(mut self, message: &str) -> Self {
			self.remove_error = Some(message.to_owned());
			self
		}

		fn refresh_error(message: &str) -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: Some(message.to_owned()),
				identifier_error: None,
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn identifier_error(message: &str) -> Self {
			Self {
				issues: Vec::new(),
				refresh_error: None,
				identifier_error: Some(message.to_owned()),
				remove_error: None,
				comments: Vec::new(),
				refresh_queries: RefCell::new(Vec::new()),
				label_removals: RefCell::new(Vec::new()),
			}
		}
	}
	impl IssueTracker for GhostLaneTestTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			Ok(self.issues.iter().filter(|issue| issue.has_label(label_name)).cloned().collect())
		}

		fn find_team_label_id(
			&self,
			team_id: &str,
			label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(self
				.issues
				.iter()
				.find(|issue| issue.team.id == team_id)
				.and_then(|issue| issue.label_id_for_name(label_name).map(ToOwned::to_owned)))
		}

		fn get_issue_by_identifier(
			&self,
			issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			if let Some(message) = &self.identifier_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(self
				.issues
				.iter()
				.find(|issue| issue.identifier.eq_ignore_ascii_case(issue_identifier))
				.cloned())
		}

		fn refresh_issues(
			&self,
			issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			self.refresh_queries.borrow_mut().push(issue_ids.to_vec());

			if let Some(message) = &self.refresh_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(self
				.issues
				.iter()
				.filter(|issue| issue_ids.iter().any(|issue_id| issue_id == &issue.id))
				.cloned()
				.collect())
		}

		fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
			Ok(self.comments.clone())
		}

		fn update_issue_state(
			&self,
			_issue_id: &str,
			_state_id: &str,
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			issue_id: &str,
			label_ids: &[String],
		) -> crate::prelude::Result<()> {
			self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));
			if let Some(message) = &self.remove_error {
				return Err(crate::prelude::eyre::eyre!(message.clone()));
			}

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

	struct FinalNeedsAttentionTracker {
		issue: TrackerIssue,
		needs_attention_label: String,
		get_issue_calls: RefCell<usize>,
		label_removals: RefCell<Vec<(String, Vec<String>)>>,
	}
	impl FinalNeedsAttentionTracker {
		fn new(issue: TrackerIssue, needs_attention_label: String) -> Self {
			Self {
				issue,
				needs_attention_label,
				get_issue_calls: RefCell::new(0),
				label_removals: RefCell::new(Vec::new()),
			}
		}

		fn issue_for_call(&self, call_count: usize) -> TrackerIssue {
			let mut issue = self.issue.clone();

			if call_count >= 3 {
				let label = TrackerLabel {
					id: format!("label-{}", self.needs_attention_label.replace(':', "-")),
					name: self.needs_attention_label.clone(),
				};
				if !issue.team.labels.iter().any(|candidate| candidate.name == label.name) {
					issue.team.labels.push(label.clone());
				}
				if !issue.labels.iter().any(|candidate| candidate.name == label.name) {
					issue.labels.push(label);
				}
			}

			issue
		}
	}
	impl IssueTracker for FinalNeedsAttentionTracker {
		fn list_issues_with_label(
			&self,
			label_name: &str,
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			let issue = self.issue_for_call(*self.get_issue_calls.borrow());

			Ok(issue.has_label(label_name).then_some(issue).into_iter().collect())
		}

		fn find_team_label_id(
			&self,
			_team_id: &str,
			label_name: &str,
		) -> crate::prelude::Result<Option<String>> {
			Ok(Some(format!("label-{}", label_name.replace(':', "-"))))
		}

		fn get_issue_by_identifier(
			&self,
			issue_identifier: &str,
		) -> crate::prelude::Result<Option<TrackerIssue>> {
			let mut calls = self.get_issue_calls.borrow_mut();

			*calls += 1;
			let issue = self.issue_for_call(*calls);

			Ok((issue.identifier == issue_identifier).then_some(issue))
		}

		fn refresh_issues(
			&self,
			issue_ids: &[String],
		) -> crate::prelude::Result<Vec<TrackerIssue>> {
			let issue = self.issue_for_call(*self.get_issue_calls.borrow());

			Ok(issue_ids
				.iter()
				.any(|issue_id| issue_id == &issue.id)
				.then_some(issue)
				.into_iter()
				.collect())
		}

		fn list_comments(&self, _issue_id: &str) -> crate::prelude::Result<Vec<TrackerComment>> {
			Ok(Vec::new())
		}

		fn update_issue_state(
			&self,
			_issue_id: &str,
			_state_id: &str,
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn add_issue_labels(
			&self,
			_issue_id: &str,
			_label_ids: &[String],
		) -> crate::prelude::Result<()> {
			Ok(())
		}

		fn remove_issue_labels(
			&self,
			issue_id: &str,
			label_ids: &[String],
		) -> crate::prelude::Result<()> {
			self.label_removals.borrow_mut().push((issue_id.to_owned(), label_ids.to_vec()));

			Ok(())
		}

		fn create_comment(&self, _issue_id: &str, _body: &str) -> crate::prelude::Result<()> {
			Ok(())
		}
	}

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

	fn sample_recovery_context(
		temp_dir: &TempDir,
		runtime_mutation_policy: super::RecoveryRuntimeMutationPolicy,
	) -> super::RecoveryContext {
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

		super::RecoveryContext {
			config: ServiceConfig::from_path(&config_path).expect("config should load"),
			workflow: sample_workflow(),
			state_store: StateStore::open_in_memory().expect("state store should open"),
			tracker: LinearClient::new(String::from("test-token"))
				.expect("linear client should build"),
			runtime_mutation_policy,
		}
	}

	fn seed_mcp_test_fixture_ghost_lane(store: &StateStore, worktree_root: &Path) {
		let channel_path = worktree_root.join("missing-run-control.channel");
		let protocol_activity = ProtocolActivitySummary {
			turn_status: Some(String::from("completed")),
			waiting_reason: Some(String::from("turn_completed")),
			rate_limit_status: None,
			recent_events: Vec::new(),
		};

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store.update_run_thread("run-12", "thread-12").expect("thread should record");
		store.update_run_turn("run-12", "turn-12").expect("turn should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-12",
				1,
				&channel_path,
				"local_file",
			)
			.expect("control channel row should publish");
		store
			.append_event("run-12", 1, "turn/completed", r#"{"status":"completed"}"#)
			.expect("protocol event should record");
		store
			.record_run_activity_summary("run-12", 1, None, Some(&protocol_activity))
			.expect("protocol activity should record");

		append_mcp_test_control_private_events(store);
	}

	fn append_mcp_test_control_private_events(store: &StateStore) {
		for (event_type, payload) in [
			(
				"control_action",
				serde_json::json!({
					"schema": "decodex.run_control_action/v1",
					"source": "mcp-test",
					"action": "steer"
				}),
			),
			(
				"control_action",
				serde_json::json!({
					"schema": "decodex.run_control_action/v1",
					"source": "cli",
					"action": "interrupt",
					"requested": {
						"project_id": "pubfi",
						"issue_id": "PUB-012",
						"run_id": "run-12",
						"attempt_number": 1,
						"thread_id": "thread-12",
						"turn_id": "turn-12"
					}
				}),
			),
			(
				"lane_control/steer/requested",
				serde_json::json!({
					"source": "mcp-test",
					"method": "turn/steer"
				}),
			),
			(
				"lane_control/interrupt/requested",
				serde_json::json!({
					"source": "mcp-test",
					"method": "turn/interrupt"
				}),
			),
		] {
			store
				.append_private_execution_event(
					"pubfi", "PUB-012", "run-12", 1, event_type, payload,
				)
				.expect("mcp-test private evidence should record");
		}
	}

	fn append_mcp_test_fixture_ghost_lane_cleanup_audit(store: &StateStore) {
		store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				GHOST_LANE_CLEANUP_EVENT,
				serde_json::json!({
					"schema": "decodex.ghost_lane_recovery_private_event/1",
					"event": GHOST_LANE_CLEANUP_EVENT,
					"classification": MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION,
					"reason": "tracker_issue_missing_and_only_mcp_test_control_fixture_evidence",
					"issue_identifier": "PUBFI-012",
					"terminal_status": GHOST_LANE_TERMINAL_STATUS,
					"cleared_run_lease": true,
					"evidence": [
						"tracker_issue_missing",
						"worktree_mapping_path_missing",
						"worktree_missing",
						"control_channel_file_missing",
						"mcp_test_fixture_control_channel_row_present",
						"mcp_test_fixture_private_control_evidence_present",
						"review_lineage_missing"
					],
					"blockers": [],
					"next_action": "ordinary automation may continue after status readback confirms no current attention lane"
				}),
			)
			.expect("cleanup audit should record");
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

	fn init_git_repo(path: &Path) {
		fs::create_dir_all(path).expect("git repo path should create");
		let status = Command::new("git")
			.arg("-C")
			.arg(path)
			.arg("init")
			.status()
			.expect("git init should run");

		assert!(status.success(), "git init should succeed");
	}

	fn commit_test_file(path: &Path, file_name: &str, body: &str, message: &str) {
		fs::write(path.join(file_name), body).expect("test file should write");
		run_git(path, &["add", file_name]);
		run_git(
			path,
			&[
				"-c",
				"user.name=Decodex Test",
				"-c",
				"user.email=decodex-test@example.invalid",
				"commit",
				"-m",
				message,
			],
		);
	}

	fn init_clean_git_repo_with_remote_default(path: &Path, branch_name: &str) {
		init_git_repo(path);
		run_git(path, &["checkout", "-B", "main"]);
		commit_test_file(path, "README.md", "base\n", "base");
		run_git(path, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
		run_git(path, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
		run_git(path, &["checkout", "-B", branch_name]);
	}

	#[test]
	fn stale_active_diagnose_classifies_tracker_present_active_without_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store.update_run_thread("run-1626", "thread-stale").expect("thread should record");
		store.update_run_turn("run-1626", "turn-stale").expect("turn should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert_eq!(
			diagnostic.reason,
			"tracker_issue_has_stale_active_label_without_live_or_retained_progress"
		);
		assert!(diagnostic.active_label_present);
		assert!(diagnostic.queue_label_present);
		assert!(!diagnostic.run_lease);
		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-1626"));
		assert!(diagnostic.blockers.is_empty(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_present")));
		assert!(diagnostic.evidence.contains(&String::from("run_lease_missing")));
		assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
		assert!(diagnostic.evidence.contains(&String::from("stale_thread_reference_present")));
		assert!(diagnostic.next_action.contains("recover stale-active release PUB-1626 --dry-run"));
	}

	#[test]
	fn stale_active_diagnose_blocks_shared_claim_lock_file() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let owner_store = StateStore::open_in_memory().expect("owner store should open");
		let store = StateStore::open_in_memory().expect("reader store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		owner_store
			.configure_dispatch_slot_root("pubfi", temp_dir.path())
			.expect("owner store should configure dispatch root");
		assert!(
			owner_store
				.try_acquire_lease("pubfi", &issue.id, "run-live", "In Progress")
				.expect("owner should acquire shared claim")
		);
		store
			.observe_dispatch_slot_root("pubfi", temp_dir.path())
			.expect("reader store should observe dispatch root");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.active_shared_claim);
		assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_run_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.upsert_lease("pubfi", &issue.identifier, "run-identifier", "In Progress")
			.expect("identifier-keyed lease should record");
		store
			.record_run_attempt("run-identifier", &issue.identifier, 1, "running")
			.expect("identifier-keyed run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.identifier,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("identifier-keyed worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-identifier"));
		assert!(diagnostic.run_lease);
		assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_private_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_private_execution_event(
				"pubfi",
				&issue.identifier,
				"run-identifier",
				1,
				"source_progress",
				serde_json::json!({"phase": "implementation"}),
			)
			.expect("identifier-keyed private progress should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_worktree_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("identifier-worktree");

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("identifier worktree should create");
		fs::write(worktree_path.join("source.rs"), "fn progress() {}\n")
			.expect("ordinary worktree file should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.identifier,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("identifier-keyed worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "non_git_files_present");
		assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("worktree path should create");
		state::write_run_thread_status_marker(
			&worktree_path,
			"run-1626",
			1,
			Some("thread-1626"),
			Some("turn-1626"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("activity_marker_thread_active")));
		assert!(!diagnostic.recoverable());
	}

	fn dead_orphan_activity_summaries() -> (ChildAgentActivitySummary, ProtocolActivitySummary) {
		(
			ChildAgentActivitySummary {
				event_count: 531,
				current_bucket: Some(String::from("Model")),
				..ChildAgentActivitySummary::default()
			},
			ProtocolActivitySummary {
				turn_status: Some(String::from("running")),
				waiting_reason: Some(String::from("model_execution")),
				..ProtocolActivitySummary::default()
			},
		)
	}

	fn seed_dead_orphan_runtime_telemetry(
		store: &StateStore,
		issue: &TrackerIssue,
		worktree_path: &Path,
	) {
		let control_channel_path = worktree_path.join(".decodex-run-control/run-1626-1.channel");
		let (child_activity, protocol_activity) = dead_orphan_activity_summaries();

		init_clean_git_repo_with_remote_default(worktree_path, "x/pubfi-pub-1626");
		state::write_run_activity_marker_for_process(worktree_path, "run-1626", 1, u32::MAX)
			.expect("stale process marker should write");
		state::write_run_protocol_activity_marker(
			worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-1626",
				attempt_number: 1,
				thread_id: Some("thread-stale"),
				turn_id: Some("turn-stale"),
				event_count: 531,
				last_event_type: "item/started",
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			},
		)
		.expect("stale protocol marker should write");
		state::write_run_thread_status_marker(
			worktree_path,
			"run-1626",
			1,
			Some("thread-stale"),
			Some("turn-stale"),
			"active",
			&[],
		)
		.expect("stale thread marker should write");
		fs::create_dir_all(control_channel_path.parent().expect("channel parent"))
			.expect("control directory should create");
		fs::write(
			&control_channel_path,
			"schema=decodex.run_control_channel/v1\nrun_id=run-1626\nattempt_number=1\n",
		)
		.expect("control channel file should write");
		store.record_run_attempt("run-1626", &issue.id, 1, "running").expect("run attempt");
		store
			.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
			.expect("temporary lease should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-1626",
				1,
				&control_channel_path,
				"local_file",
			)
			.expect("control channel should publish");
		store.clear_lease(&issue.id).expect("stale lane lease should clear");
		store
			.append_event("run-1626", 1, "item/started", r#"{"kind":"model"}"#)
			.expect("protocol event should record");
		store
			.record_run_activity_summary(
				"run-1626",
				1,
				Some(&child_activity),
				Some(&protocol_activity),
			)
			.expect("activity summary should record");
		append_dead_orphan_private_telemetry(store, &issue.id);
	}

	fn append_dead_orphan_private_telemetry(store: &StateStore, issue_id: &str) {
		for (event_type, payload) in [
			(
				"control_channel_published",
				serde_json::json!({
					"schema": "decodex.run_control_channel/v1",
					"status": "active",
				}),
			),
			(
				"phase_goal_set",
				serde_json::json!({
					"schema": "decodex.phase_goal_signal/1",
					"phase": "implement_to_validation_ready",
				}),
			),
			(
				"progress_checkpoint",
				serde_json::json!({
					"phase": "probing",
					"pr_url": null,
					"verification": [],
					"head_sha": "1111111111111111111111111111111111111111",
				}),
			),
			(
				"control_action",
				serde_json::json!({
					"action": "interrupt",
					"reason": "run_lease_missing",
				}),
			),
			(
				"lane_control/interrupt",
				serde_json::json!({
					"classification": "hard_interrupt_fallback",
					"processAliveAfter": false,
					"signals": [],
					"status": "sent",
				}),
			),
		] {
			store
				.append_private_execution_event(
					"pubfi", issue_id, "run-1626", 1, event_type, payload,
				)
				.expect("private stale telemetry should record");
		}
	}

	#[test]
	fn stale_active_diagnose_allows_dead_orphan_thread_runtime_telemetry() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let queue_label = tracker::automation_queue_label("pubfi");
		let mut issue = sample_issue_with_labels("In Progress", &[active_label, queue_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		seed_dead_orphan_runtime_telemetry(&store, &issue, &worktree_path);

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, STALE_ACTIVE_CLASSIFICATION);
		assert!(diagnostic.recoverable(), "unexpected blockers: {:?}", diagnostic.blockers);
		assert!(diagnostic.evidence.contains(&String::from("process_not_alive")));
		assert!(
			diagnostic.evidence.contains(&String::from("stale_active_control_channel_present"))
		);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("only_stale_active_or_failed_control_evidence_present"))
		);
	}

	#[test]
	fn stale_active_diagnose_blocks_clean_worktree_with_unmerged_commits() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		run_git(&worktree_path, &["checkout", "-B", "main"]);
		commit_test_file(&worktree_path, "README.md", "base\n", "base");
		run_git(&worktree_path, &["checkout", "-b", "x/pubfi-pub-1626"]);
		commit_test_file(&worktree_path, "source.rs", "fn retained_progress() {}\n", "progress");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "unmerged_commits_present");
		assert!(diagnostic.blockers.contains(&String::from("worktree_unmerged_commits_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_clean_git_worktree_without_default_branch() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "default_branch_unavailable");
		assert!(
			diagnostic
				.blockers
				.contains(&String::from("worktree_default_branch_unavailable"))
		);
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_private_progress_from_older_attempt() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-old", &issue.id, 1, "running")
			.expect("old run attempt should record");
		store
			.record_run_attempt("run-new", &issue.id, 2, "running")
			.expect("new run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_private_execution_event(
				"pubfi",
				&issue.id,
				"run-old",
				1,
				"source_progress",
				serde_json::json!({"phase": "implementation"}),
			)
			.expect("private progress should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.latest_run_id.as_deref(), Some("run-new"));
		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("private_progress_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_protocol_event_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.append_event("run-1626", 1, "turn/item", r#"{"kind":"progress"}"#)
			.expect("protocol event should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("protocol_event_evidence_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_marker_protocol_activity_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");
		state::write_run_protocol_activity_marker(
			&worktree_path,
			&ProtocolActivityMarker {
				run_id: "run-1626",
				attempt_number: 1,
				thread_id: Some("thread-stale"),
				turn_id: Some("turn-stale"),
				event_count: 1,
				last_event_type: "turn/completed",
				child_agent_activity: None,
				protocol_activity: None,
			},
		)
		.expect("protocol marker should write");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("protocol_event_marker_present")));
		assert!(
			diagnostic
				.blockers
				.contains(&String::from("activity_marker_protocol_activity_present"))
		);
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_untracked_worktree_progress() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_git_repo(&worktree_path);
		fs::write(worktree_path.join("new_source.rs"), "fn retained_progress() {}\n")
			.expect("untracked source should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_non_git_retained_files() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&worktree_path).expect("retained path should create");
		fs::write(worktree_path.join("retained.txt"), "retained work\n")
			.expect("retained file should write");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "non_git_files_present");
		assert!(diagnostic.blockers.contains(&String::from("non_git_worktree_files_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_child_agent_activity_summary() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.record_run_activity_summary("run-1626", 1, Some(&activity), None)
			.expect("child activity should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_when_worktree_status_unknown() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let worktree_path = temp_dir.path().join("PUB-1626");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		fs::create_dir_all(&worktree_path).expect("worktree path should create");
		fs::write(worktree_path.join(".git"), "gitdir: /does/not/exist\n")
			.expect("invalid gitdir should write");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.worktree_state, "tracked_changes_unknown");
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_needs_attention_label() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let needs_attention_label = String::from("decodex:needs-attention");
		let mut issue = sample_issue_with_labels("Todo", &[active_label, needs_attention_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("needs_attention_label_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_review_policy_checkpoint() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: &issue.id,
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("review checkpoint should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_review_policy_checkpoint() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("identifier-keyed review checkpoint should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_pr_lineage() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "PUB-1626",
				issue_identifier: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
			},
			"review_handoff",
			String::from("2026-06-28T00:00:00Z"),
			"review_handoff",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		event.branch = Some(String::from("x/pubfi-pub-1626"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
		event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
		event.validation_result = Some(String::from("passed"));
		event.summary = Some(String::from("Recorded review handoff lineage."));
		event.terminal_path = Some(String::from("review_handoff"));
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store.record_linear_execution_event(&event).expect("linear event should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_tracker_comment_pr_lineage() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "linear-issue-1626",
				issue_identifier: "PUB-1626",
				run_id: "run-1626",
				attempt_number: 1,
			},
			"review_handoff",
			String::from("2026-06-28T00:00:00Z"),
			"review_handoff",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		event.branch = Some(String::from("x/pubfi-pub-1626"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/1626"));
		event.pr_head_sha = Some(String::from("2222222222222222222222222222222222222222"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("3333333333333333333333333333333333333333"));
		event.validation_result = Some(String::from("passed"));
		event.summary = Some(String::from("Recorded review handoff lineage."));
		event.terminal_path = Some(String::from("review_handoff"));

		let comment = TrackerComment {
			body: records::append_structured_comment_record(
				&records::render_linear_execution_event_comment_body(&event, None),
				&event,
			)
			.expect("structured comment should serialize"),
			created_at: String::from("2026-06-28T00:00:00Z"),
		};

		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]).with_comments(vec![comment]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_identifier_keyed_review_lifecycle() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let marker = ReviewHandoffMarker::new(
			"run-1626",
			1,
			"x/pubfi-pub-1626",
			"https://github.com/hack-ink/decodex/pull/1626",
			"main",
			"x/pubfi-pub-1626",
			"2222222222222222222222222222222222222222",
		);

		issue.id = String::from("linear-issue-1626");
		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&temp_dir.path().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");
		store
			.upsert_review_handoff_marker("pubfi", "PUB-1626", &marker)
			.expect("identifier-keyed review lifecycle should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_diagnose_blocks_unreadable_activity_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);
		let worktree_path = temp_dir.path().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(worktree_path.join(state::RUN_ACTIVITY_MARKER_FILE))
			.expect("directory marker should create");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_worktree(
				"pubfi",
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("worktree_tracked_changes_unknown")));
		assert!(
			diagnostic.evidence.iter().any(|entry| entry.starts_with("worktree_status_error:")),
			"diagnostic should include marker read error evidence: {:?}",
			diagnostic.evidence
		);
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn stale_active_release_removes_active_label_and_terminalizes_stale_run() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("stale active release should apply");

		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
			.expect("private events should read");

		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
		assert!(events.iter().any(|event| {
			event.event_type() == STALE_ACTIVE_RELEASE_EVENT
				&& event.payload()["schema"] == super::STALE_ACTIVE_RECOVERY_SCHEMA
				&& event.payload()["active_label_release"] == "pending_final_mutation"
				&& event.payload()["phase"] == "local_cleanup_complete_before_active_label_release"
		}));
	}

	#[test]
	fn stale_active_release_removes_run_control_marker_only_directory() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");
		let control_dir = worktree_path.join(state::RUN_CONTROL_CHANNEL_DIR);

		issue.identifier = String::from("PUB-1626");
		fs::create_dir_all(&control_dir).expect("run-control marker directory should create");
		fs::write(control_dir.join("run-1626-1.channel"), "channel\n")
			.expect("run-control marker should write");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect("stale active release should apply");

		assert!(!worktree_path.exists(), "marker-only directory should be removed");
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_keeps_active_label_gate_when_tracker_label_removal_fails() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()])
			.remove_error("Linear label removal failed");
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("tracker removal failure should abort release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", &issue.id, "run-1626", 1)
			.expect("private events should read");
		let mapping = context
			.state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree mapping should read");

		assert!(error.to_string().contains("Linear label removal failed"));
		assert_eq!(run.status(), GHOST_LANE_TERMINAL_STATUS);
		assert!(events.iter().any(|event| event.event_type() == STALE_ACTIVE_RELEASE_EVENT));
		assert!(mapping.is_none());
		assert_eq!(
			tracker.label_removals.borrow().as_slice(),
			&[(issue.id.clone(), vec![format!("label-{}", active_label.replace(':', "-"))])]
		);
	}

	#[test]
	fn stale_active_release_revalidates_needs_attention_before_final_label_removal() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let needs_attention_label =
			context.workflow.frontmatter().tracker().needs_attention_label().to_owned();
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = FinalNeedsAttentionTracker::new(issue, needs_attention_label);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("initial stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late needs-attention should block active-label release");
		let message = error.to_string();

		assert!(message.contains("safety inspection changed before apply"));
		assert!(message.contains("needs_attention_label_present"));
		assert!(
			tracker.label_removals.borrow().is_empty(),
			"active label should not be removed after late needs-attention appears"
		);
	}

	#[test]
	fn stale_active_release_preflight_rejects_worktree_progress_after_diagnosis() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);
		let worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		init_clean_git_repo_with_remote_default(&worktree_path, "x/pubfi-pub-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&worktree_path.display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		fs::write(worktree_path.join("late_progress.rs"), "fn late_progress() {}\n")
			.expect("late untracked progress should write");
		let error =
			super::preflight_stale_active_worktree_cleanup(&context.state_store, &diagnostic)
				.expect_err("preflight should reject late retained progress");

		assert!(
			error.to_string().contains("retained worktree changes appeared before cleanup"),
			"unexpected preflight error: {error:?}"
		);
	}

	#[test]
	fn stale_active_release_revalidates_late_default_worktree_progress_without_mapping() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);
		let default_worktree_path = context.config.worktree_root().join("PUB-1626");

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		init_git_repo(&default_worktree_path);
		fs::write(default_worktree_path.join("late_default_progress.rs"), "fn late() {}\n")
			.expect("late default progress should write");
		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late default worktree progress should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_release_revalidates_late_run_lease_before_mutation() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label.clone(), queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
			.expect("late lease should record");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late run lease should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_release_revalidates_late_review_policy_before_mutation() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				&issue.id,
				"x/pubfi-pub-1626",
				&context.config.worktree_root().join("PUB-1626").display().to_string(),
			)
			.expect("worktree mapping should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: context.config.service_id(),
				issue_id: &issue.id,
				run_id: "run-1626",
				attempt_number: 1,
				phase: "handoff",
				review_level: "normal",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("late review checkpoint should record");

		let error = super::apply_stale_active_release_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("late review checkpoint should block release");
		let run = context
			.state_store
			.run_attempt("run-1626")
			.expect("run attempt should read")
			.expect("run should exist");

		assert!(
			error.to_string().contains("safety inspection changed before apply")
				|| error.to_string().contains("review authority appeared"),
			"unexpected release error: {error:?}"
		);
		assert_eq!(run.status(), "running");
		assert!(tracker.label_removals.borrow().is_empty());
	}

	#[test]
	fn stale_active_final_label_guard_rejects_late_run_lease() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context = sample_recovery_context(
			&temp_dir,
			super::RecoveryRuntimeMutationPolicy::AllowRuntimeWrites,
		);
		let active_label = tracker::automation_active_label(context.config.service_id());
		let queue_label = tracker::automation_queue_label(context.config.service_id());
		let mut issue = sample_issue_with_labels("Todo", &[active_label, queue_label]);

		issue.identifier = String::from("PUB-1626");
		context
			.state_store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
		let mut diagnostics = super::diagnose_stale_active_issues(
			context.config.service_id(),
			&context.workflow,
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		context
			.state_store
			.upsert_lease(context.config.service_id(), &issue.id, "run-1626", "In Progress")
			.expect("late lease should record");

		let error = super::ensure_stale_active_run_claim_guard(
			&context.config,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("final guard should reject late lease");

		assert!(
			error.to_string().contains("appeared before active-label release"),
			"unexpected final guard error: {error:?}"
		);
	}

	#[test]
	fn stale_active_diagnose_blocks_when_run_lease_is_present() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let workflow = sample_workflow();
		let active_label = tracker::automation_active_label("pubfi");
		let mut issue = sample_issue_with_labels("Todo", &[active_label]);

		issue.identifier = String::from("PUB-1626");
		store
			.record_run_attempt("run-1626", &issue.id, 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", &issue.id, "run-1626", "In Progress")
			.expect("lease should record");

		let tracker = GhostLaneTestTracker::with_issues(vec![issue]);
		let diagnostics = super::diagnose_stale_active_issues(
			"pubfi",
			&workflow,
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-1626"),
			super::RecoveryRuntimeMutationPolicy::ReadOnly,
		)
		.expect("stale active diagnosis should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, super::STALE_ACTIVE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("run_lease_present")));
		assert!(diagnostic.blockers.contains(&String::from("active_shared_claim_present")));
		assert!(!diagnostic.recoverable());
	}

	#[test]
	fn recovery_read_only_backoff_observer_does_not_clear_expired_backoff() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let expired_unix_epoch = time::OffsetDateTime::now_utc().unix_timestamp() - 1;

		context
			.state_store
			.upsert_connector_backoff(ConnectorBackoffInput {
				project_id: context.config.service_id(),
				connector: "linear",
				sync_phase: "ghost_lane_recovery",
				quota_class: "linear_graphql_rate_limit",
				reset_unix_epoch: expired_unix_epoch,
				reset_source: "test",
				warning: super::LINEAR_RATE_LIMIT_BACKOFF_WARNING,
			})
			.expect("backoff should persist");

		let message = super::active_recovery_tracker_backoff_message(&context)
			.expect("backoff observer should run");

		assert_eq!(message, None);
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_some(),
			"read-only recovery diagnostics must not clear stored connector backoff"
		);
	}

	#[test]
	fn recovery_read_only_backoff_recorder_does_not_persist_new_backoff() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let error = crate::prelude::eyre::eyre!("Linear connector timed out while testing");
		let message = super::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		)
		.expect("timeout should produce backoff message");

		assert!(message.contains("Linear connector is in backoff"));
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_none(),
			"read-only recovery diagnostics must not persist new connector backoff"
		);
	}

	#[test]
	fn review_handoff_diagnose_skips_terminal_identifier_worktree_before_tracker_refresh() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let stale_issue_id = "PUB-001";
		let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

		context
			.state_store
			.record_run_attempt("run-01", stale_issue_id, 1, super::GHOST_LANE_TERMINAL_STATUS)
			.expect("terminal attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				stale_issue_id,
				"x/pubfi-pub-001",
				&stale_worktree_path.display().to_string(),
			)
			.expect("stale worktree mapping should record");

		let diagnostics =
			super::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
				.expect("retained review diagnostics should build");
		let diagnostic = diagnostics.first().expect("local residue diagnostic should render");

		assert_eq!(diagnostics.len(), 1);
		assert_eq!(
			diagnostic.classification,
			super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
		);
		assert_eq!(diagnostic.issue_id, stale_issue_id);
		assert_eq!(diagnostic.issue_state, "local_terminal_residue");
		assert!(
			tracker.refresh_queries.borrow().is_empty(),
			"terminal local identifier residue must not be sent to tracker refresh"
		);
	}

	#[test]
	fn review_handoff_diagnose_targeted_terminal_identifier_worktree_before_tracker_lookup() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::identifier_error(
			"Linear GraphQL request failed: Argument Validation Error",
		);
		let stale_issue_id = "PUB-001";
		let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

		context
			.state_store
			.record_run_attempt("run-01", stale_issue_id, 1, super::GHOST_LANE_TERMINAL_STATUS)
			.expect("terminal attempt should record");
		context
			.state_store
			.upsert_worktree(
				context.config.service_id(),
				stale_issue_id,
				"x/pubfi-pub-001",
				&stale_worktree_path.display().to_string(),
			)
			.expect("stale worktree mapping should record");

		let diagnostic = super::diagnose_issue_with_tracker(&context, &tracker, stale_issue_id)
			.expect("targeted retained review diagnostic should classify local residue");

		assert_eq!(
			diagnostic.classification,
			super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
		);
		assert_eq!(diagnostic.issue_id, stale_issue_id);
		assert_eq!(diagnostic.issue_state, "local_terminal_residue");
		assert!(
			tracker.refresh_queries.borrow().is_empty(),
			"targeted terminal local residue must not be sent to tracker refresh"
		);
	}

	#[test]
	fn ghost_lane_live_status_overlay_tracker_backoff_stays_read_only() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let missing_tracker = GhostLaneTestTracker::missing();
		let error_tracker =
			GhostLaneTestTracker::refresh_error("Linear connector timed out while testing");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let mut diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&missing_tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let error = super::apply_ghost_lane_live_status_blockers_with_tracker(
			&error_tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&mut diagnostics,
		)
		.expect_err("overlay tracker error should surface for recovery backoff wrapping");
		let message = super::remember_recovery_tracker_backoff_message(
			&context,
			&error,
			"ghost_lane_recovery",
		)
		.expect("timeout should become a recovery backoff message");

		assert!(message.contains("ghost_lane_recovery"));
		assert!(
			context
				.state_store
				.connector_backoff(context.config.service_id(), "linear")
				.expect("backoff should read")
				.is_none(),
			"read-only live-status overlay must not persist connector backoff"
		);
	}

	#[test]
	fn ghost_lane_diagnose_live_status_overlay_blocks_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = context.config.worktree_root().join("PUB-012");
		let mut diagnostics = vec![super::GhostLaneDiagnostic {
			project_id: String::from("pubfi"),
			issue_id: String::from("PUB-012"),
			issue_identifier: Some(String::from("PUBFI-012")),
			run_id: String::from("run-12"),
			attempt_number: 1,
			attempt_status: String::from("running"),
			classification: String::from(GHOST_LANE_CLASSIFICATION),
			reason: String::from("test"),
			run_lease: true,
			control_channel: String::from("missing"),
			evidence: Vec::new(),
			blockers: Vec::new(),
			next_action: String::from("test"),
		}];

		fs::create_dir_all(&worktree_path).expect("worktree path should exist");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		context
			.state_store
			.upsert_worktree(
				"pubfi",
				"PUB-012",
				"x/pubfi-pub-012",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_thread_status_marker(
			&worktree_path,
			"run-12",
			1,
			Some("thread-12"),
			Some("turn-12"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");
		super::apply_ghost_lane_live_status_blockers_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&mut diagnostics,
		)
		.expect("status overlay should run");

		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(diagnostic.blockers.contains(&String::from("status:thread_active")));
		assert!(diagnostic.blockers.contains(&String::from("status:retained_worktree_present")));
	}

	#[test]
	fn ghost_lane_cleanup_live_status_gate_rejects_active_thread_marker() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = context.config.worktree_root().join("PUB-012");
		let diagnostic = super::GhostLaneDiagnostic {
			project_id: String::from("pubfi"),
			issue_id: String::from("PUB-012"),
			issue_identifier: Some(String::from("PUBFI-012")),
			run_id: String::from("run-12"),
			attempt_number: 1,
			attempt_status: String::from("running"),
			classification: String::from(GHOST_LANE_CLASSIFICATION),
			reason: String::from("test"),
			run_lease: true,
			control_channel: String::from("missing"),
			evidence: Vec::new(),
			blockers: Vec::new(),
			next_action: String::from("test"),
		};

		fs::create_dir_all(&worktree_path).expect("worktree path should exist");

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		context
			.state_store
			.upsert_worktree(
				"pubfi",
				"PUB-012",
				"x/pubfi-pub-012",
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");

		state::write_run_thread_status_marker(
			&worktree_path,
			"run-12",
			1,
			Some("thread-12"),
			Some("turn-12"),
			"active",
			&[String::from("waitingOnApproval")],
		)
		.expect("active thread marker should write");

		let error = super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			&diagnostic,
		)
		.expect_err("live status should reject cleanup");
		let message = format!("{error:#}");

		assert!(message.contains("live status reported blockers"));
		assert!(message.contains("thread_active"));
		assert!(message.contains("retained_worktree_present"));
	}

	#[test]
	fn ghost_lane_cleanup_terminalizes_missing_issue_lease_and_records_private_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let mut diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.pop().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert_eq!(diagnostic.issue_id, "PUB-012");
		assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
		assert!(diagnostic.evidence.contains(&String::from("worktree_missing")));
		assert!(diagnostic.evidence.contains(&String::from("control_channel_missing")));
		assert!(diagnostic.evidence.contains(&String::from("private_evidence_missing")));
		assert!(diagnostic.evidence.contains(&String::from("review_lineage_missing")));

		super::apply_ghost_lane_cleanup(&store, &diagnostic).expect("cleanup should apply");

		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").is_empty(),
			"cleanup should clear the local run lease"
		);

		let runs =
			store.list_project_issue_runs("pubfi", "PUB-012").expect("issue runs should load");

		assert_eq!(runs.len(), 1);
		assert_eq!(runs[0].status(), GHOST_LANE_TERMINAL_STATUS);

		let events = store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should load");

		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_type(), GHOST_LANE_CLEANUP_EVENT);
		assert_eq!(
			events[0].payload()["schema"].as_str(),
			Some("decodex.ghost_lane_recovery_private_event/1")
		);
		assert_eq!(events[0].payload()["cleared_run_lease"].as_bool(), Some(true));
	}

	#[test]
	fn ghost_lane_cleanup_dry_run_validation_keeps_runtime_state_untouched() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		context
			.state_store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		context
			.state_store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert!(diagnostic.recoverable());

		super::ensure_ghost_lane_live_status_allows_cleanup_with_tracker(
			&tracker,
			&context.config,
			&context.workflow,
			&context.state_store,
			diagnostic,
		)
		.expect("dry-run validation should allow cleanup");

		let runs = context
			.state_store
			.list_project_issue_runs("pubfi", "PUB-012")
			.expect("issue runs should load");
		let events = context
			.state_store
			.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
			.expect("private events should load");

		assert_eq!(runs.len(), 1);
		assert_eq!(runs[0].status(), "running");
		assert!(
			!context
				.state_store
				.list_leased_runs("pubfi")
				.expect("leased runs should load")
				.is_empty(),
			"dry-run validation must not clear the run lease"
		);
		assert!(events.is_empty(), "dry-run validation must not write cleanup audit events");
	}

	#[test]
	fn ghost_lane_diagnostic_allows_mcp_test_fixture_control_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("mcp-test fixture ghost lane should diagnose");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.blockers.is_empty());
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
		);
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_protocol_or_thread_evidence_present"))
		);
	}

	#[test]
	fn ghost_lane_diagnostic_allows_prior_mcp_test_fixture_cleanup_audit() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());
		append_mcp_test_fixture_ghost_lane_cleanup_audit(&context.state_store);

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("prior cleanup audit should not block an idempotent diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, MCP_TEST_FIXTURE_GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.blockers.is_empty());
		assert!(
			diagnostic
				.evidence
				.contains(&String::from("mcp_test_fixture_private_control_evidence_present"))
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_mcp_fixture_has_mixed_private_evidence() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let context =
			sample_recovery_context(&temp_dir, super::RecoveryRuntimeMutationPolicy::ReadOnly);
		let tracker = GhostLaneTestTracker::missing();

		seed_mcp_test_fixture_ghost_lane(&context.state_store, context.config.worktree_root());

		context
			.state_store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				"progress_checkpoint",
				serde_json::json!({"source": "runtime", "phase": "implementing"}),
			)
			.expect("real private evidence should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			context.config.service_id(),
			context.config.worktree_root(),
			&context.state_store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
	}

	#[test]
	fn ghost_lane_diagnostic_treats_invalid_local_issue_id_refresh_as_missing_issue() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::refresh_error(
			"Linear GraphQL request failed: Argument Validation Error",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("missing local issue id should not abort ghost-lane diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
	}

	#[test]
	fn ghost_lane_diagnostic_treats_missing_identifier_lookup_as_missing_issue() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::identifier_error(
			"Linear GraphQL request failed: Entity not found: Issue",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes_read_only(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("missing issue identifier should not abort ghost-lane diagnosis");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_CLASSIFICATION);
		assert!(diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("tracker_issue_missing")));
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_requested_issue_identifier_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let mut issue = sample_issue("In Progress");

		issue.id = String::from("linear-pubfi-012");
		issue.identifier = String::from("PUBFI-012");

		let tracker = GhostLaneTestTracker {
			issues: vec![issue],
			refresh_error: None,
			identifier_error: None,
			remove_error: None,
			comments: Vec::new(),
			refresh_queries: RefCell::new(Vec::new()),
			label_removals: RefCell::new(Vec::new()),
		};

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert_eq!(diagnostic.issue_identifier.as_deref(), Some("PUBFI-012"));
		assert!(diagnostic.blockers.contains(&String::from("tracker_issue_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_rejects_unrelated_requested_identifier() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "ABC-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "ABC-012", "run-12", "In Progress")
			.expect("lease should record");

		let error = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect_err("unrelated issue prefixes should not match by numeric suffix alone");

		assert!(
			format!("{error:#}").contains("No leased lane matched"),
			"unexpected error: {error:#}"
		);
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"failed selector matches must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_requested_worktree_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = temp_dir.path().join("PUBFI-012");

		fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUBFI-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_control_channel_row_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let channel_path = temp_dir.path().join("missing-control-channel.json");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.publish_run_control_channel_for_active_attempt(
				"run-12",
				1,
				&channel_path,
				"local_file",
			)
			.expect("control channel row should publish");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.evidence.contains(&String::from("control_channel_file_missing")));
		assert!(diagnostic.blockers.contains(&String::from("control_channel_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_private_evidence_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.append_private_execution_event(
				"pubfi",
				"PUB-012",
				"run-12",
				1,
				"diagnostic",
				serde_json::json!({"schema": "test.private/1"}),
			)
			.expect("private evidence should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("private_evidence_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_review_lifecycle_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let marker = ReviewHandoffMarker::new(
			"run-12",
			1,
			"x/pubfi-pub-012",
			"https://github.com/hack-ink/decodex/pull/12",
			"main",
			"x/pubfi-pub-012",
			"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		);

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.upsert_review_handoff_marker("pubfi", "PUB-012", &marker)
			.expect("review lifecycle should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("review_lifecycle_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_review_checkpoint_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: "pubfi",
				issue_id: "PUB-012",
				run_id: "run-12",
				attempt_number: 1,
				phase: "handoff",
				review_level: "standard",
				status: "clean",
				head_sha: "2222222222222222222222222222222222222222",
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("review checkpoint should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("review_policy_checkpoint_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_pr_lineage_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let mut event = LinearExecutionEventRecord::new(
			LinearExecutionEventIdentity {
				service_id: "pubfi",
				issue_id: "PUB-012",
				issue_identifier: "PUB-012",
				run_id: "run-12",
				attempt_number: 1,
			},
			"closeout",
			String::from("2026-06-18T00:00:00Z"),
			"closeout",
		);

		event.branch = Some(String::from("x/pubfi-pub-012"));
		event.pr_url = Some(String::from("https://github.com/hack-ink/decodex/pull/12"));
		event.pr_head_sha = Some(String::from("08a20f7dfb9526e7421a5f095b1c6adec84e52d6"));
		event.pr_base_ref = Some(String::from("main"));
		event.commit_sha = Some(String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d7"));
		event.summary = Some(String::from("Recorded retained closeout."));

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");
		store.record_linear_execution_event(&event).expect("linear event should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("pr_or_review_lineage_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_retained_worktree_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let worktree_path = temp_dir.path().join("PUB-012");

		fs::create_dir_all(&worktree_path).expect("retained worktree should exist");

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("retained_worktree_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
	}

	#[test]
	fn ghost_lane_diagnostic_fails_closed_when_activity_summary_exists() {
		let temp_dir = TempDir::new().expect("tempdir should create");
		let store = StateStore::open_in_memory().expect("state store should open");
		let tracker = GhostLaneTestTracker::missing();
		let activity = ChildAgentActivitySummary { event_count: 1, ..Default::default() };

		store
			.record_run_attempt("run-12", "PUB-012", 1, "running")
			.expect("run attempt should record");
		store
			.record_run_activity_summary("run-12", 1, Some(&activity), None)
			.expect("activity summary should record");
		store
			.upsert_lease("pubfi", "PUB-012", "run-12", "In Progress")
			.expect("lease should record");

		let diagnostics = super::diagnose_ghost_lanes(
			"pubfi",
			temp_dir.path(),
			&store,
			&tracker,
			Some("PUB-012"),
		)
		.expect("ghost lane diagnostic should run");
		let diagnostic = diagnostics.first().expect("diagnostic should exist");

		assert_eq!(diagnostic.classification, GHOST_LANE_BLOCKED_CLASSIFICATION);
		assert!(!diagnostic.recoverable());
		assert!(diagnostic.blockers.contains(&String::from("child_agent_activity_present")));
		assert!(
			store.list_leased_runs("pubfi").expect("leased runs should load").len() == 1,
			"fail-closed diagnostics must preserve attention"
		);
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
	fn adopt_landing_state_rejects_blocked_merge_state_after_green_gates() {
		let mut landing_state = sample_landing_state(
			"https://github.com/hack-ink/decodex/pull/344",
			"xy/xy-944-manual-takeover-adopt",
			"1123456789abcdef0123456789abcdef01234567",
		);

		landing_state.merge_state_status = String::from("BLOCKED");

		let error = super::validate_adopt_landing_state(&landing_state)
			.expect_err("manual takeover should not bypass blocked merge state");

		assert!(error.to_string().contains("not ready to adopt"));
		assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
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
	fn adopt_private_event_records_manual_takeover_lifecycle_evidence() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let validation = super::AdoptValidation {
			issue: sample_issue("In Review"),
			branch_name: branch_name.to_owned(),
			worktree_path: Path::new("/tmp/PUB-718").to_path_buf(),
			run_id: String::from("pub-718-manual-adopt-2-1123456789ab"),
			attempt_number: 2,
			landing_state: sample_landing_state(pr_url, branch_name, head_oid),
			local_head_oid: head_oid.to_owned(),
			worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
			active_label_present: false,
			success_state_transition: None,
			previous_worktree_mapping: None,
		};

		super::append_review_handoff_adopt_private_event(
			&state_store,
			"pubfi",
			&validation,
			"local_markers_written",
			false,
		)
		.expect("adopt private event should append");
		super::append_review_handoff_adopt_private_event(
			&state_store,
			"pubfi",
			&validation,
			"active_label_checked",
			true,
		)
		.expect("adopt active-label private event should append");

		let events = state_store
			.list_private_execution_events(
				"pubfi",
				&validation.issue.id,
				&validation.run_id,
				validation.attempt_number,
			)
			.expect("private events should read");
		let event = events.first().expect("adopt event should exist");
		let payload = event.payload();
		let second_event = events.get(1).expect("active-label adopt event should exist");
		let second_payload = second_event.payload();

		assert_eq!(events.len(), 2);
		assert_eq!(event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
		assert_eq!(payload["event"], REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(payload["writeback_stage"], "local_markers_written");
		assert_eq!(payload["manual_takeover_adopt"], true);
		assert_eq!(payload["active_label_restored"], false);
		assert_eq!(payload["pr_url"], pr_url);
		assert_eq!(payload["pr_head_sha"], head_oid);
		assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
		assert_eq!(second_event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
		assert_eq!(second_payload["writeback_stage"], "active_label_checked");
		assert_eq!(second_payload["active_label_restored"], true);
	}

	#[test]
	fn rebind_private_event_records_retained_lifecycle_evidence() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
		let validation = super::RebindValidation {
			issue: sample_issue("In Review"),
			worktree: sample_worktree(branch_name),
			run_id: String::from("pub-718-attempt-2-1123456789ab"),
			attempt_number: 2,
			landing_state: sample_landing_state(pr_url, branch_name, head_oid),
			local_head_oid: head_oid.to_owned(),
			worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
			active_label_present: true,
			restore_active_label: false,
			mode: super::RebindMode::RefreshExistingHandoff,
			success_state_transition: None,
			clear_needs_attention_label: false,
		};

		super::append_review_handoff_rebind_private_event(
			&state_store,
			"pubfi",
			&validation,
			"local_markers_written",
			false,
		)
		.expect("rebind private event should append");

		let events = state_store
			.list_private_execution_events(
				"pubfi",
				&validation.issue.id,
				&validation.run_id,
				validation.attempt_number,
			)
			.expect("private events should read");
		let event = events.first().expect("rebind event should exist");
		let payload = event.payload();

		assert_eq!(events.len(), 1);
		assert_eq!(event.event_type(), REVIEW_HANDOFF_REBIND_EVENT);
		assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
		assert_eq!(payload["event"], REVIEW_HANDOFF_REBIND_EVENT);
		assert_eq!(payload["writeback_stage"], "local_markers_written");
		assert_eq!(payload["mode"], "refresh_existing_handoff");
		assert_eq!(payload["active_label_present"], true);
		assert_eq!(payload["active_label_restored"], false);
		assert_eq!(payload["pr_url"], pr_url);
		assert_eq!(payload["pr_head_sha"], head_oid);
		assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
	}

	#[test]
	fn rebind_lifecycle_marker_write_failure_clears_partial_handoff_marker() {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let branch_name = "x/pubfi-pub-718";
		let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
		let head_oid = "1123456789abcdef0123456789abcdef01234567";
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

		let error = super::write_review_lifecycle_markers_with_rollback(
			&state_store,
			"pubfi",
			"issue-id",
			&handoff,
			&orchestration,
			|| -> crate::prelude::Result<()> {
				Err(crate::prelude::eyre::eyre!("orchestration marker write failed"))
			},
		)
		.expect_err("orchestration write failure should be returned");

		assert!(error.to_string().contains("orchestration marker write failed"));
		assert!(
			state_store
				.review_lifecycle_record("pubfi", "issue-id", branch_name)
				.expect("lifecycle read should succeed")
				.is_none()
		);
		assert!(
			state_store
				.review_handoff_marker("pubfi", "issue-id", branch_name)
				.expect("handoff read should succeed")
				.is_none()
		);
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
			failure_state: "Todo",
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
			failure_state: "Todo",
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
			failure_state: "Todo",
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
			failure_state: "Todo",
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
			failure_state: "Todo",
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
	fn diagnostic_reports_rebind_for_failure_state_ownership_drift() {
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
			issue_state_name: "Todo",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
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
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
		assert!(!diagnostic.next_action.contains("Restore explicit lane ownership"));
	}

	#[test]
	fn diagnostic_reports_rebind_for_failure_state_drift_with_active_label() {
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
			issue_state_name: "Todo",
			success_state: "In Review",
			in_progress_state: "In Progress",
			failure_state: "Todo",
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
		assert_eq!(diagnostic.reason, "review_handoff_failure_state_drift");
		assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
		assert!(diagnostic.next_action.contains("rebind PUB-718"));
		assert!(diagnostic.next_action.contains("--dry-run"));
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
		record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));
		record.evidence = Some(vec![String::from("existing_review_lifecycle_record=absent")]);

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
		record.summary = Some(String::from("Explicit operator rebind restored lifecycle record."));

		let error = records::validate_linear_execution_event_record(&record)
			.expect_err("rebind event without evidence should fail");

		assert!(error.contains("evidence"));
	}
}
