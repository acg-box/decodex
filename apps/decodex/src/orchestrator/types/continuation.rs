use crate::tracker;

use super::{
	IssueDispatchMode, IssueTracker, PullRequestReviewStateInspector, RetainedCloseoutPrMergeGate,
	TrackerIssue, TrackerToolBridge, TurnContinuationGuard, WorkflowDocument, eyre, refresh_issue,
	retained_closeout_pr_merge_gate_with_inspector,
};

pub(crate) struct IssueTurnContinuationGuard<'a, T> {
	pub(crate) tracker: &'a T,
	pub(crate) tracker_tool_bridge: &'a TrackerToolBridge<'a>,
	pub(crate) workflow: &'a WorkflowDocument,
	pub(crate) service_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) issue_identifier: &'a str,
	pub(crate) initial_issue_state: &'a str,
	#[allow(dead_code)]
	#[cfg(test)]
	pub(crate) retry_project_slug: &'a str,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) review_state_inspector: Option<&'a dyn PullRequestReviewStateInspector>,
}
impl<T> IssueTurnContinuationGuard<'_, T>
where
	T: IssueTracker,
{
	pub(crate) fn issue_has_service_ownership(
		&self,
		issue: &TrackerIssue,
	) -> crate::prelude::Result<bool> {
		tracker::issue_has_label_with_server_confirmation(
			self.tracker,
			issue,
			&tracker::automation_active_label(self.service_id),
		)
	}

	pub(crate) fn completed_closeout_pr_is_merged(&self) -> crate::prelude::Result<bool> {
		let Some(review_state_inspector) = self.review_state_inspector else {
			return Ok(false);
		};
		let Some(review_context) = self.tracker_tool_bridge.review_context() else {
			return Ok(false);
		};
		let Some(pr_url) = review_context.recorded_pr_url.as_deref() else {
			return Ok(false);
		};

		match retained_closeout_pr_merge_gate_with_inspector(
			&review_context.cwd,
			&review_context.branch_name,
			pr_url,
			review_state_inspector,
		)? {
			RetainedCloseoutPrMergeGate::Merged => Ok(true),
			RetainedCloseoutPrMergeGate::NotMerged => Ok(false),
			RetainedCloseoutPrMergeGate::PullRequestStateReadFailed => {
				eyre::bail!(
					"retained closeout PR state read failed while validating the continuation boundary"
				)
			},
		}
	}
}

impl<T> TurnContinuationGuard for IssueTurnContinuationGuard<'_, T>
where
	T: IssueTracker,
{
	fn should_continue_turn(&self, _turn_count: u32) -> crate::prelude::Result<bool> {
		let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
			return Ok(false);
		};
		let tracker_policy = self.workflow.frontmatter().tracker();

		if !self.issue_has_service_ownership(&issue)? {
			return Ok(false);
		}
		if self.dispatch_mode == IssueDispatchMode::ReviewRepair {
			return Ok(issue.state.name == tracker_policy.success_state()
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label()));
		}
		if self.dispatch_mode == IssueDispatchMode::Closeout {
			let completed_state = tracker_policy.resolved_completed_state();

			return Ok((issue.state.name == tracker_policy.success_state()
				|| (issue.state.name == completed_state
					&& self.completed_closeout_pr_is_merged()?))
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label()));
		}

		let issue_remains_active = issue.state.name == tracker_policy.in_progress_state()
			&& !issue.has_label(tracker_policy.opt_out_label())
			&& !issue.has_label(tracker_policy.needs_attention_label());

		if issue_remains_active {
			return Ok(true);
		}

		let stale_startup_snapshot =
			self.tracker_tool_bridge.startup_transition_succeeded_locally()
				&& issue.state.name == self.initial_issue_state
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label());

		Ok(stale_startup_snapshot)
	}

	fn validate_continuation_boundary(&self, turn_count: u32) -> crate::prelude::Result<()> {
		if self.dispatch_mode == IssueDispatchMode::ReviewRepair {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();

			if issue.state.name == tracker_policy.success_state()
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
			{
				return Ok(());
			}

			eyre::bail!(
				"Turn {} for issue `{}` ended without keeping the tracker issue in `{}`; a clean {} continuation boundary is only valid while the lane remains in its retained post-review state.",
				turn_count,
				self.issue_identifier,
				tracker_policy.success_state(),
				"retained review-repair",
			);
		}
		if self.dispatch_mode == IssueDispatchMode::Closeout {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();
			let completed_state = tracker_policy.resolved_completed_state();
			let issue_completed_with_merged_pr =
				issue.state.name == completed_state && self.completed_closeout_pr_is_merged()?;

			if (issue.state.name == tracker_policy.success_state()
				|| issue_completed_with_merged_pr)
				&& !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
			{
				return Ok(());
			}

			let retained_states =
				format!("`{}` or `{}`", tracker_policy.success_state(), completed_state);

			eyre::bail!(
				"Turn {} for issue `{}` ended without keeping the tracker issue in {}; a clean retained closeout continuation boundary is only valid while the lane remains in its retained post-review state.",
				turn_count,
				self.issue_identifier,
				retained_states,
			);
		}
		if turn_count != 1 {
			return Ok(());
		}
		if self.tracker_tool_bridge.startup_transition_succeeded_locally() {
			let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
				return Ok(());
			};
			let tracker_policy = self.workflow.frontmatter().tracker();

			if !issue.has_label(tracker_policy.opt_out_label())
				&& !issue.has_label(tracker_policy.needs_attention_label())
				&& (issue.state.name == tracker_policy.in_progress_state()
					|| issue.state.name == self.initial_issue_state)
			{
				return Ok(());
			}
		}

		let Some(issue) = refresh_issue(self.tracker, self.issue_id)? else {
			return Ok(());
		};
		let in_progress = self.workflow.frontmatter().tracker().in_progress_state();

		if issue.state.name != in_progress {
			eyre::bail!(
				"Turn 1 for issue `{}` ended without moving the tracker issue to `{}`; a clean continuation boundary is only valid after the startup transition succeeds.",
				self.issue_identifier,
				in_progress
			);
		}

		Ok(())
	}
}
