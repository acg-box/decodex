use crate::{
	config::ServiceConfig,
	orchestrator::{PostReviewLaneSnapshot, PullRequestReviewState},
	state::{ReviewOrchestrationMarker, StateStore},
	workflow::WorkflowDocument,
};

pub(crate) struct RetainedReviewLane {
	pub(super) snapshot: PostReviewLaneSnapshot,
	pub(super) review_state: PullRequestReviewState,
	pub(super) orchestration_marker: ReviewOrchestrationMarker,
}

pub(crate) struct PassiveRetainedAttentionRuntime<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) project: &'a ServiceConfig,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) state_store: &'a StateStore,
}
impl<T> Clone for PassiveRetainedAttentionRuntime<'_, T> {
	fn clone(&self) -> Self {
		*self
	}
}

impl<T> Copy for PassiveRetainedAttentionRuntime<'_, T> {}

pub(super) struct RetainedReviewRuntime<'a, T> {
	pub(super) tracker: &'a T,
	pub(super) project: &'a ServiceConfig,
	pub(super) workflow: &'a WorkflowDocument,
	pub(super) state_store: &'a StateStore,
	pub(super) github_token: &'a mut Option<String>,
	pub(super) now_unix_epoch: i64,
}

#[derive(Clone, Copy)]
pub(super) struct RetainedReviewOrchestrationMarkerFields {
	pub(super) request_comment_database_id: Option<i64>,
	pub(super) request_created_at_unix_epoch: Option<i64>,
	pub(super) request_retry_count: i64,
	pub(super) external_round_count: i64,
	pub(super) auto_merge_enabled_at_unix_epoch: Option<i64>,
}
impl RetainedReviewOrchestrationMarkerFields {
	pub(super) fn from_marker(marker: &ReviewOrchestrationMarker) -> Self {
		Self {
			request_comment_database_id: marker.request_comment_database_id(),
			request_created_at_unix_epoch: marker.request_created_at_unix_epoch(),
			request_retry_count: marker.request_retry_count(),
			external_round_count: marker.external_round_count(),
			auto_merge_enabled_at_unix_epoch: marker.auto_merge_enabled_at_unix_epoch(),
		}
	}
}

#[derive(Clone, Copy)]
pub(super) struct RetainedAdminMergeReasons {
	pub(super) start_landing: &'static str,
	pub(super) admin_merge_unavailable: &'static str,
	pub(super) admin_merge_failed: &'static str,
}

pub(super) enum RetainedReviewLaneReviewLoad {
	Skip,
	Blocked(String),
	ReviewState(Box<PullRequestReviewState>),
}
