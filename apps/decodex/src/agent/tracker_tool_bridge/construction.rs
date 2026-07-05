use std::cell::RefCell;

#[cfg(test)]
use crate::tracker::privacy_classifier::DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER;
use crate::{
	agent::tracker_tool_bridge::{
		GH_PULL_REQUEST_INSPECTOR, LOCAL_GIT_REPO_INSPECTOR, LocalRepoInspector,
		PullRequestInspector, ReviewHandoffContext, TrackerToolBridge,
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

impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn with_run_context_state_store_and_privacy_classifier(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: &'a StateStore,
		public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(state_store),
				public_projection_privacy_classifier,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
			},
		)
	}

	pub(crate) fn review_context(&self) -> Option<&ReviewHandoffContext> {
		self.review_context.as_ref()
	}

	pub(crate) fn manual_attention_error_class(&self) -> Option<String> {
		self.manual_attention_error_class.borrow().clone()
	}
}

#[cfg(test)]
impl<'a> TrackerToolBridge<'a> {
	pub(crate) fn new(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
	) -> Self {
		Self {
			tracker,
			issue,
			workflow,
			review_context: None,
			state_store: None,
			public_projection_privacy_classifier: &DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
			pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
			local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
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

	pub(in crate::agent::tracker_tool_bridge) fn with_review_handoff_inspectors(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: Option<&'a StateStore>,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store,
				public_projection_privacy_classifier:
					&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
				pull_request_inspector,
				local_repo_inspector,
			},
		)
	}

	pub(in crate::agent::tracker_tool_bridge) fn leaked_test_state_store() -> &'static StateStore {
		Box::leak(Box::new(
			StateStore::open_in_memory().expect("test runtime state store should open"),
		))
	}

	pub(crate) fn with_review_handoff_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			pull_request_inspector,
			local_repo_inspector,
		)
	}

	pub(crate) fn with_review_repair_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		pull_request_inspector: &'a dyn PullRequestInspector,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			pull_request_inspector,
			local_repo_inspector,
		)
	}

	pub(crate) fn with_run_context(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
	) -> Self {
		Self::with_review_handoff_inspectors(
			tracker,
			issue,
			workflow,
			review_context,
			Some(Self::leaked_test_state_store()),
			&GH_PULL_REQUEST_INSPECTOR,
			&LOCAL_GIT_REPO_INSPECTOR,
		)
	}

	pub(crate) fn with_run_context_and_state_store(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		state_store: &'a StateStore,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(state_store),
				public_projection_privacy_classifier:
					&DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector: &LOCAL_GIT_REPO_INSPECTOR,
			},
		)
	}

	pub(crate) fn with_review_handoff_classifier_for_test(
		tracker: &'a dyn IssueTracker,
		issue: &'a TrackerIssue,
		workflow: &'a WorkflowDocument,
		review_context: ReviewHandoffContext,
		public_projection_privacy_classifier: &'a dyn PublicProjectionPrivacyClassifier,
		local_repo_inspector: &'a dyn LocalRepoInspector,
	) -> Self {
		Self::with_review_handoff_options(
			tracker,
			issue,
			workflow,
			review_context,
			TrackerToolBridgeOptions {
				state_store: Some(Self::leaked_test_state_store()),
				public_projection_privacy_classifier,
				pull_request_inspector: &GH_PULL_REQUEST_INSPECTOR,
				local_repo_inspector,
			},
		)
	}
}
