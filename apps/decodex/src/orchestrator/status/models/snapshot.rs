use crate::{
	config::{ReviewLevel, ServiceConfig},
	orchestrator::{
		GhPullRequestReviewStateInspector,
		status_models::gates::{AccountActivityMode, RunIssueMetadataHydration},
	},
	state::StateStore,
	workflow::WorkflowDocument,
};

#[derive(Clone, Copy)]
pub(in crate::orchestrator) struct LiveOperatorStatusSnapshotOptions {
	pub(in crate::orchestrator) hydrate_history_ledger: bool,
	pub(in crate::orchestrator) run_issue_metadata_hydration: RunIssueMetadataHydration,
	pub(in crate::orchestrator) account_activity_mode: AccountActivityMode,
	pub(in crate::orchestrator) configure_dispatch_slots: bool,
}

#[derive(Clone, Copy)]
pub(in crate::orchestrator) struct PostReviewRuntimeState<'a> {
	pub(in crate::orchestrator) state_store: &'a StateStore,
	pub(in crate::orchestrator) project_id: &'a str,
	pub(in crate::orchestrator) review_level: ReviewLevel,
}
pub(in crate::orchestrator) struct LiveOperatorStatusObserverContext<'a, T> {
	pub(in crate::orchestrator) tracker: &'a T,
	pub(in crate::orchestrator) project: &'a ServiceConfig,
	pub(in crate::orchestrator) workflow: &'a WorkflowDocument,
	pub(in crate::orchestrator) state_store: &'a StateStore,
	pub(in crate::orchestrator) review_state_inspector: &'a GhPullRequestReviewStateInspector,
	pub(in crate::orchestrator) hydrate_history_ledger: bool,
	pub(in crate::orchestrator) run_issue_metadata_hydration: RunIssueMetadataHydration,
}

pub(in crate::orchestrator) struct PostReviewLaneBuildContext<'a, I> {
	pub(in crate::orchestrator) project: &'a ServiceConfig,
	pub(in crate::orchestrator) workflow: &'a WorkflowDocument,
	pub(in crate::orchestrator) state_store: &'a StateStore,
	pub(in crate::orchestrator) review_state_inspector: &'a I,
	pub(in crate::orchestrator) success_state: &'a str,
	pub(in crate::orchestrator) completed_state: &'a str,
}
