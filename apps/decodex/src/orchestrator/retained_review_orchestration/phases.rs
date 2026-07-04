mod merge;
mod non_github;
mod request;
mod result;

use crate::orchestrator::retained_review_orchestration::{
	IssueTracker, Result, RetainedReviewLane, RetainedReviewRuntime, ReviewOrchestrationPhase,
	ServiceConfig, StateStore, WorkflowDocument, eyre,
};

pub(super) fn reconcile_retained_review_lane<T>(
	tracker: &T,
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
	lane: &RetainedReviewLane,
	github_token: &mut Option<String>,
	now_unix_epoch: i64,
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
		);
	}

	let phase =
		ReviewOrchestrationPhase::parse(lane.orchestration_marker.phase()).map_err(|error| {
			eyre::eyre!("Failed to parse retained review orchestration phase: {error}")
		})?;

	match phase {
		ReviewOrchestrationPhase::RequestPending =>
			request::handle_request_pending_phase(project, state_store, lane, github_token),
		ReviewOrchestrationPhase::WaitingForAck => request::handle_waiting_for_ack_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			github_token,
			now_unix_epoch,
		),
		ReviewOrchestrationPhase::WaitingForResult
		| ReviewOrchestrationPhase::PassWaitingForGates => {
			let mut runtime = RetainedReviewRuntime {
				tracker,
				project,
				workflow,
				state_store,
				github_token,
				now_unix_epoch,
			};

			result::handle_waiting_for_result_phase(&mut runtime, lane, phase)
		},
		ReviewOrchestrationPhase::RepairRequired => Ok(()),
		ReviewOrchestrationPhase::WaitingForMerge => merge::handle_waiting_for_merge_phase(
			tracker,
			project,
			workflow,
			state_store,
			lane,
			now_unix_epoch,
			"external_review_merge_visibility_timeout",
		),
	}
}
