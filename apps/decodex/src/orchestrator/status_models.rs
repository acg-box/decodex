use std::collections::HashMap;

use crate::{
	config::{ReviewLevel, ServiceConfig},
	orchestrator::{
		self, GhPullRequestReviewStateInspector, OperatorExecutionProgramStatus,
		OperatorHistoryLedgerOutcome, OperatorReviewRouteCount, PostReviewLaneClassification,
		PostReviewLaneDecision, PullRequestReadbackRootCause, PullRequestReviewState,
		ReviewOrchestrationPhase, TrackerConnectorBackoff,
	},
	prelude::{Result, eyre},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore},
	tracker::records::LinearExecutionEventRecord,
	workflow::WorkflowDocument,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetainedCloseoutPrMergeGate {
	Merged,
	NotMerged,
	PullRequestStateReadFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExternalReviewRequestCiGate {
	Ready,
	WaitForGreenChecks,
	RepairRequired,
}

#[derive(Clone, Copy)]
pub(super) enum AccountActivityMode {
	Probe,
	Snapshot,
}

#[derive(Clone, Copy)]
pub(super) enum RunIssueMetadataHydration {
	AllRows,
	CurrentLaneRowsOnly,
}

pub(super) enum TrackerObserverOutcome {
	Ok,
	Unavailable,
	Backoff(TrackerConnectorBackoff),
}

#[derive(Clone, Copy)]
pub(super) struct LiveOperatorStatusSnapshotOptions {
	pub(super) hydrate_history_ledger: bool,
	pub(super) run_issue_metadata_hydration: RunIssueMetadataHydration,
	pub(super) account_activity_mode: AccountActivityMode,
	pub(super) configure_dispatch_slots: bool,
}

#[derive(Clone, Copy)]
pub(super) struct PostReviewRuntimeState<'a> {
	pub(super) state_store: &'a StateStore,
	pub(super) project_id: &'a str,
	pub(super) review_level: ReviewLevel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PostReviewReadbackDegradation<'a> {
	pub(super) reason: &'a str,
	pub(super) root_cause: PullRequestReadbackRootCause,
	pub(super) pr_url: &'a str,
	pub(super) pr_head_sha: &'a str,
}
impl<'a> PostReviewReadbackDegradation<'a> {
	pub(super) fn tracker_issue_from_handoff(review_handoff: &'a ReviewHandoffMarker) -> Self {
		Self {
			reason: "tracker_issue_readback_degraded",
			root_cause: PullRequestReadbackRootCause::TrackerIssueReadbackFailed,
			pr_url: review_handoff.pr_url(),
			pr_head_sha: review_handoff.pr_head_oid(),
		}
	}

	pub(super) fn pull_request_state_from_handoff(
		review_handoff: &'a ReviewHandoffMarker,
		root_cause: PullRequestReadbackRootCause,
	) -> Self {
		Self {
			reason: "pull_request_state_read_failed",
			root_cause,
			pr_url: review_handoff.pr_url(),
			pr_head_sha: review_handoff.pr_head_oid(),
		}
	}

	pub(super) fn wait_for_review_classification(
		self,
		review_state: Option<PullRequestReviewState>,
	) -> PostReviewLaneClassification {
		let (
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
		) = match review_state {
			Some(review_state) => (
				Some(review_state.head_ref_oid),
				Some(review_state.state),
				review_state.review_decision,
				Some(review_state.mergeable),
				review_state.status_check_rollup_state,
				Some(review_state.unresolved_review_threads),
			),
			None => (Some(self.pr_head_sha.to_owned()), None, None, None, None, None),
		};

		PostReviewLaneClassification {
			decision: PostReviewLaneDecision::WaitForReview,
			reason: self.reason.to_owned(),
			pr_url: Some(self.pr_url.to_owned()),
			pr_head_sha,
			pr_state,
			review_decision,
			mergeable,
			check_state,
			unresolved_review_threads,
			readback_warning: Some(self.reason.to_owned()),
			readback_root_cause: Some(self.root_cause.as_str().to_owned()),
		}
	}
}

pub(super) struct PostReviewOrchestrationStatus {
	pub(super) phase: ReviewOrchestrationPhase,
	pub(super) request_acknowledged: bool,
	pub(super) review_result_arrived: bool,
	pub(super) strict_pass: bool,
	pub(super) clean_path_landing_gates_satisfied: bool,
	pub(super) landing_requires_agent_fallback: bool,
}
impl PostReviewOrchestrationStatus {
	pub(super) fn from_review_state(
		review_state: &PullRequestReviewState,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> Result<Self> {
		let phase =
			ReviewOrchestrationPhase::parse(orchestration_marker.phase()).map_err(|error| {
				eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
			})?;

		Ok(Self {
			phase,
			request_acknowledged: orchestrator::request_comment_has_eyes(
				review_state,
				orchestration_marker,
			)
			.unwrap_or(false),
			review_result_arrived: orchestrator::external_review_result_arrived(
				review_state,
				orchestration_marker,
			),
			strict_pass: orchestrator::external_review_has_strict_pass_signals(
				review_state,
				orchestration_marker,
			),
			clean_path_landing_gates_satisfied:
				orchestrator::review_state_clean_path_landing_gates_satisfied(review_state),
			landing_requires_agent_fallback:
				orchestrator::review_state_landing_requires_agent_fallback(review_state),
		})
	}
}

pub(super) struct OperatorRunTiming {
	pub(super) process_id: Option<u32>,
	pub(super) process_alive: Option<bool>,
	pub(super) process_liveness_reason: Option<String>,
	pub(super) last_run_activity_unix_epoch: Option<i64>,
	pub(super) last_protocol_activity_unix_epoch: Option<i64>,
	pub(super) last_progress_unix_epoch: Option<i64>,
	pub(super) idle_for_seconds: Option<i64>,
	pub(super) protocol_idle_for_seconds: Option<i64>,
}

#[derive(Clone, Copy)]
pub(super) struct MarkerProcessLiveness {
	pub(super) alive: bool,
	pub(super) reason: &'static str,
}

pub(super) struct OperatorRunAppServerState {
	pub(super) thread_id: Option<String>,
	pub(super) turn_id: Option<String>,
	pub(super) thread_status: Option<String>,
	pub(super) thread_active_flags: Vec<String>,
	pub(super) interactive_requested: bool,
	pub(super) continuation_pending: bool,
	pub(super) effective_model: Option<String>,
	pub(super) effective_model_provider: Option<String>,
	pub(super) effective_cwd: Option<String>,
	pub(super) effective_approval_policy: Option<String>,
	pub(super) effective_approvals_reviewer: Option<String>,
	pub(super) effective_sandbox_mode: Option<String>,
}

pub(super) struct OperatorRunProtocolSummary {
	pub(super) last_event_type: Option<String>,
	pub(super) last_event_at: Option<String>,
	pub(super) event_count: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct OperatorTerminalFinalizeProjection {
	pub(super) status: &'static str,
	pub(super) phase: &'static str,
	pub(super) wait_reason: &'static str,
	pub(super) current_operation: &'static str,
}

pub(super) struct OperatorRunLifecycleProjection {
	pub(super) status: String,
	pub(super) status_projection_reason: Option<String>,
	pub(super) phase: String,
	pub(super) wait_reason: Option<String>,
	pub(super) current_operation: String,
	pub(super) suspected_stall: bool,
	pub(super) execution_liveness: String,
	pub(super) run_lease: bool,
	pub(super) retry_kind: Option<String>,
	pub(super) retry_ready_at_unix_epoch: Option<i64>,
}

pub(super) struct LiveOperatorStatusObserverContext<'a, T> {
	pub(super) tracker: &'a T,
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) review_state_inspector: &'a GhPullRequestReviewStateInspector,
	pub(super) hydrate_history_ledger: bool,
	pub(super) run_issue_metadata_hydration: RunIssueMetadataHydration,
}

pub(super) struct PostReviewLaneBuildContext<'a, I> {
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) review_state_inspector: &'a I,
	pub(super) success_state: &'a str,
	pub(super) completed_state: &'a str,
}

pub(super) struct OperatorHistoryLedgerRecord {
	pub(super) record: LinearExecutionEventRecord,
	pub(super) event_unix_epoch: Option<i64>,
	pub(super) sort_unix_epoch: Option<i64>,
	pub(super) comment_index: usize,
}

pub(super) struct OperatorIssueDisplayMetadata {
	pub(super) issue_identifier: String,
	pub(super) title: Option<String>,
	pub(super) author: Option<String>,
	pub(super) issue_state: Option<String>,
	pub(super) active_label_present: Option<bool>,
	pub(super) needs_attention_label_present: Option<bool>,
}

pub(super) struct WorktreeOwnership {
	pub(super) kind: &'static str,
	pub(super) reason: String,
	pub(super) next_action: Option<String>,
	pub(super) audit_required: bool,
}

pub(super) struct OperatorLifecycleMetricPhase {
	pub(super) key: &'static str,
	pub(super) label: &'static str,
	pub(super) rank: u8,
}

pub(super) struct OperatorLaneControlProjection {
	pub(super) ownership_state: String,
	pub(super) liveness_state: String,
	pub(super) policy_state: String,
	pub(super) terminalization_state: String,
	pub(super) next_action: String,
	pub(super) conditions: Vec<String>,
}

#[derive(Default)]
pub(super) struct OperatorLaneTerminalProjection {
	pub(super) outcomes_by_issue_key: HashMap<String, OperatorHistoryLedgerOutcome>,
}

pub(super) struct OperatorExecutionProgramReadback {
	pub(super) statuses: Vec<OperatorExecutionProgramStatus>,
	pub(super) issue_metadata_unavailable: bool,
}

pub(super) struct OperatorReviewCheckpointSummaryFields {
	pub(super) review_class: Option<String>,
	pub(super) risk_class: Option<String>,
	pub(super) compact_eligible: Option<bool>,
	pub(super) fallback_reason: Option<String>,
	pub(super) active_fingerprints: Vec<String>,
	pub(super) stop_fingerprint: Option<String>,
	pub(super) route_counts: Vec<OperatorReviewRouteCount>,
	pub(super) route_next_action: Option<String>,
}
