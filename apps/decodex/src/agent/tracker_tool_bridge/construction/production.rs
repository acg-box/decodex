use crate::{
	agent::tracker_tool_bridge::{
		GH_PULL_REQUEST_INSPECTOR, LOCAL_GIT_REPO_INSPECTOR, ReviewHandoffContext,
		TrackerToolBridge, construction::options::TrackerToolBridgeOptions,
	},
	state::StateStore,
	tracker::{IssueTracker, TrackerIssue, privacy_classifier::PublicProjectionPrivacyClassifier},
	workflow::WorkflowDocument,
};

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
