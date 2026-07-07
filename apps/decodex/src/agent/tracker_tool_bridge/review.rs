mod closeout;
mod completion;
mod handoff;
mod lifecycle;
mod linear_events;
mod policy;
mod repair;
mod repo;

use color_eyre::Report;

use crate::{
	agent::tracker_tool_bridge::{
		self, CLOSEOUT_PUBLIC_SUMMARY_FALLBACK, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, PendingReviewAction, PendingReviewCompletion,
		PullRequestDetails, REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK,
		REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK, ReviewHandoffContext, ReviewHandoffWritebackFailed,
		RunCompletionDisposition, TrackerToolBridge,
	},
	prelude::eyre,
	tracker::records::LinearExecutionEventPublicProjection,
};
use linear_events::{
	linear_execution_closeout_event, linear_execution_review_event,
	review_lifecycle_handoff_from_pull_request, review_lifecycle_handoff_lineage_matches,
};
