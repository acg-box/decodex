mod closeout;
mod completion;
mod handoff;
mod lifecycle;
mod linear_events;
mod policy;
mod repair;
mod repo;

use color_eyre::Report;
use serde_json::Value;

use crate::{
	agent::tracker_tool_bridge::{
		self, CLOSEOUT_PUBLIC_SUMMARY_FALLBACK, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME,
		ISSUE_TRANSITION_TOOL_NAME, PendingReviewAction, PendingReviewCompletion,
		PullRequestDetails, REVIEW_HANDOFF_PUBLIC_SUMMARY_FALLBACK,
		REVIEW_REPAIR_PUBLIC_SUMMARY_FALLBACK, ReviewExecutionMode, ReviewHandoffContext,
		ReviewHandoffWritebackFailed, RunCompletionDisposition, ScopeArgs, TrackerToolBridge,
		tools::REVIEW_COMPLETION_INTENT_EVENT_TYPE,
	},
	prelude::eyre,
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
	tracker::{TrackerIssue, records::LinearExecutionEventPublicProjection},
};
use linear_events::{
	linear_execution_closeout_event, linear_execution_review_event,
	review_handoff_marker_from_pull_request, review_handoff_marker_lineage_matches,
};
