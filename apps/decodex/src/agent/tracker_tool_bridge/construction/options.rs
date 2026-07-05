use std::cell::RefCell;

use crate::{
	agent::tracker_tool_bridge::{
		LocalRepoInspector, PullRequestInspector, ReviewHandoffContext, TrackerToolBridge,
	},
	state::StateStore,
	tracker::{IssueTracker, TrackerIssue, privacy_classifier::PublicProjectionPrivacyClassifier},
	workflow::WorkflowDocument,
};

pub(in crate::agent::tracker_tool_bridge::construction) struct TrackerToolBridgeOptions<'a> {
	pub(in crate::agent::tracker_tool_bridge::construction) state_store: Option<&'a StateStore>,
	pub(in crate::agent::tracker_tool_bridge::construction) public_projection_privacy_classifier:
		&'a dyn PublicProjectionPrivacyClassifier,
	pub(in crate::agent::tracker_tool_bridge::construction) pull_request_inspector:
		&'a dyn PullRequestInspector,
	pub(in crate::agent::tracker_tool_bridge::construction) local_repo_inspector:
		&'a dyn LocalRepoInspector,
}

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::construction) fn with_review_handoff_options(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		options: TrackerToolBridgeOptions<'a>,
	) -> Self {
		Self {
			tracker,
			issue,
			workflow,
			review_context: Some(review_context),
			state_store: options.state_store,
			public_projection_privacy_classifier: options.public_projection_privacy_classifier,
			pull_request_inspector: options.pull_request_inspector,
			local_repo_inspector: options.local_repo_inspector,
			local_issue_state_name: RefCell::new(issue.state.name.clone()),
			local_opt_out_requested: RefCell::new(
				issue.has_label(workflow.frontmatter().tracker().opt_out_label()),
			),
			manual_attention_requested: RefCell::new(false),
			manual_attention_comment_recorded: RefCell::new(false),
			manual_attention_error_class: RefCell::new(None),
			continuation_blocking_tracker_write: RefCell::new(None),
			pending_review_completion: RefCell::new(None),
			finalized_completion_path: RefCell::new(None),
		}
	}
}
