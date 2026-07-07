mod merge;
mod non_github;
mod request;
mod result;

use crate::orchestrator::retained_review_orchestration::{
	IssueTracker, Result, RetainedReviewLane, RetainedReviewRuntime, ServiceConfig, StateStore,
	WorkflowDocument, eyre,
};
use crate::orchestrator::runtime_standard_review::RuntimeStandardReviewRunner;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RetainedReviewLifecycleAction {
	StartReviewGateOrExternalReview,
	RequestExternalReview,
	WaitForExternalReviewAck,
	WaitForExternalReviewResult,
	WaitForLandingGates,
	RunReviewRepair,
	PollLandingReadback,
	RunCloseoutAdapter,
	RequestManualAttention,
}
impl RetainedReviewLifecycleAction {
	fn parse(value: &str) -> Result<Self> {
		Ok(match value {
			"wait_for_runtime_review_gate_or_external_review" => {
				Self::StartReviewGateOrExternalReview
			},
			"request_external_review" => Self::RequestExternalReview,
			"wait_for_external_review_ack" => Self::WaitForExternalReviewAck,
			"wait_for_external_review_result" => Self::WaitForExternalReviewResult,
			"wait_for_landing_gates" => Self::WaitForLandingGates,
			"run_retained_review_repair_adapter" => Self::RunReviewRepair,
			"poll_landing_readback" => Self::PollLandingReadback,
			"run_retained_closeout_adapter" => Self::RunCloseoutAdapter,
			"request_manual_attention" => Self::RequestManualAttention,
			_ => eyre::bail!("Unknown retained review lifecycle action `{value}`."),
		})
	}
}

pub(super) fn reconcile_retained_review_lane<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
	runtime_review_runner: &impl RuntimeStandardReviewRunner,
) -> Result<()>
where
	T: IssueTracker,
{
	if !project.codex().review_level().uses_github_review() {
		return non_github::handle_non_github_review_lane(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			github_token,
			now_unix_epoch,
			runtime_review_runner,
		);
	}

	let action = RetainedReviewLifecycleAction::parse(lane.lifecycle_record().next_action())?;

	match action {
		RetainedReviewLifecycleAction::StartReviewGateOrExternalReview
		| RetainedReviewLifecycleAction::RequestExternalReview => {
			request::handle_request_pending_phase(project, state_store, lane, github_token)
		},
		RetainedReviewLifecycleAction::WaitForExternalReviewAck => {
			request::handle_waiting_for_ack_phase(
				tracker,
				project,
				workflow,
				state_store,
				lane,
				github_token,
				now_unix_epoch,
			)
		},
		RetainedReviewLifecycleAction::WaitForExternalReviewResult
		| RetainedReviewLifecycleAction::WaitForLandingGates => {
			let mut runtime =
				RetainedReviewRuntime { tracker, project, workflow, state_store, github_token };

			result::handle_waiting_for_result_phase(
				&mut runtime,
				lane,
				action,
				runtime_review_runner,
			)
		},
		RetainedReviewLifecycleAction::RunReviewRepair => Ok(()),
		RetainedReviewLifecycleAction::PollLandingReadback
		| RetainedReviewLifecycleAction::RunCloseoutAdapter => merge::handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"external_review_merge_visibility_timeout",
		),
		RetainedReviewLifecycleAction::RequestManualAttention => Ok(()),
	}
}
