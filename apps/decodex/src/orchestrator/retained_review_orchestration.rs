pub(in crate::orchestrator) mod attention;
pub(in crate::orchestrator) mod lifecycle_authority;

mod admin_merge;
mod command;
mod load;
mod model;
mod phases;
mod reconcile;
mod stale_worktree;

#[cfg(test)]
pub(crate) use self::reconcile::reconcile_post_review_orchestration_with_inspector_and_runtime_review_runner;
pub(crate) use self::{
	attention::apply_passive_retained_manual_attention,
	model::{PassiveRetainedAttentionRuntime, RetainedReviewLane},
	reconcile::{
		reconcile_post_review_orchestration, reconcile_post_review_orchestration_with_inspector,
	},
	stale_worktree::worktree_mapping_is_stale_terminal_local_residue,
};

use std::{path::Path, process::Command};

use color_eyre::Report;

use self::{
	command::{
		retained_review_command_adapter, retained_review_command_intent,
		retained_review_command_intent_for_issue,
	},
	model::{
		RetainedAdminMergeReasons, RetainedReviewLifecycleAuthorityFields, RetainedReviewRuntime,
	},
};
use crate::{
	config::ServiceConfig,
	github,
	orchestrator::{
		EXTERNAL_REVIEW_ACK_TIMEOUT_SECS, EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS,
		EXTERNAL_REVIEW_REQUEST_BODY, ExternalReviewRequestCiGate, IssueDispatchMode, IssueRunPlan,
		PostReviewRuntimeState, PullRequestReviewState, RetainedReviewNeedsAttention,
		RetainedReviewRunIdentity, TerminalFailureWritebackRuntime, WorktreeSpec,
		kernel::command::{CommandIntent, CommandIntentKind},
	},
	prelude::{Result, eyre},
	state::{ReviewLifecycleReadback, StateStore, WorktreeMapping},
	tracker::{IssueTracker, TrackerIssue},
	workflow::WorkflowDocument,
};
