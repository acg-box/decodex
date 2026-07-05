use crate::{
	agent::tracker_tool_bridge::{
		ISSUE_TRANSITION_TOOL_NAME, ReviewExecutionMode, ScopeArgs, TrackerToolBridge,
	},
	tracker::TrackerIssue,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn ensure_issue_scope(
		&self,
		scope: &ScopeArgs,
	) -> std::result::Result<(), String> {
		if let Some(issue_id) = scope.issue_id.as_deref()
			&& issue_id != self.issue.id
		{
			return Err(format!(
				"Tool call targeted issue id `{issue_id}`, but the leased issue id is `{}`.",
				self.issue.id
			));
		}
		if let Some(issue_identifier) = scope.issue_identifier.as_deref()
			&& issue_identifier != self.issue.identifier
		{
			return Err(format!(
				"Tool call targeted issue identifier `{issue_identifier}`, but the leased issue identifier is `{}`.",
				self.issue.identifier
			));
		}

		Ok(())
	}

	pub(in crate::agent::tracker_tool_bridge) fn allowed_transition_states(&self) -> Vec<&str> {
		let tracker = self.workflow.frontmatter().tracker();

		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Closeout)
		) {
			return vec![tracker.resolved_completed_state()];
		}

		let success_state = tracker.success_state();
		let mut states = tracker
			.startable_states()
			.iter()
			.map(String::as_str)
			.filter(|state| *state != success_state)
			.collect::<Vec<_>>();

		for state in [tracker.in_progress_state(), tracker.failure_state()] {
			if state != success_state && !states.iter().any(|existing| existing == &state) {
				states.push(state);
			}
		}

		states
	}

	pub(in crate::agent::tracker_tool_bridge) fn refreshed_issue_snapshot(
		&self,
	) -> crate::prelude::Result<Option<TrackerIssue>> {
		let issue_ids = [self.issue.id.clone()];
		let mut refreshed_issues = self.tracker.refresh_issues(&issue_ids)?;

		Ok(refreshed_issues.pop())
	}

	pub(in crate::agent::tracker_tool_bridge) fn record_continuation_blocking_transition(
		&self,
		state: &str,
	) {
		if state != self.workflow.frontmatter().tracker().in_progress_state() {
			self.record_continuation_blocking_write(format!(
				"`{ISSUE_TRANSITION_TOOL_NAME}` to state `{state}`"
			));
		}
	}

	pub(in crate::agent::tracker_tool_bridge) fn record_continuation_blocking_write(
		&self,
		reason: String,
	) {
		self.continuation_blocking_tracker_write.replace(Some(reason));
	}

	pub(in crate::agent::tracker_tool_bridge) fn local_issue_remains_active(&self) -> bool {
		self.local_issue_state_name.borrow().as_str()
			== self.workflow.frontmatter().tracker().in_progress_state()
			&& !*self.local_opt_out_requested.borrow()
			&& !*self.manual_attention_requested.borrow()
	}

	pub(in crate::agent::tracker_tool_bridge) fn continuation_blocking_write_reason(
		&self,
	) -> crate::prelude::Result<Option<String>> {
		let Some(reason) = self.continuation_blocking_tracker_write.borrow().clone() else {
			return Ok(None);
		};
		let tracker_policy = self.workflow.frontmatter().tracker();
		let run_started_active = self.issue.state.name == tracker_policy.in_progress_state();

		if run_started_active && !self.local_issue_remains_active() {
			return Ok(Some(reason));
		}

		let issue = match self.refreshed_issue_snapshot()? {
			Some(issue) => issue,
			None => return Ok(Some(reason)),
		};
		let issue_still_active = issue.state.name == tracker_policy.in_progress_state()
			&& !issue.has_label(tracker_policy.opt_out_label())
			&& !issue.has_label(tracker_policy.needs_attention_label());

		if issue_still_active {
			return Ok(None);
		}

		Ok(Some(reason))
	}

	pub(crate) fn startup_transition_succeeded_locally(&self) -> bool {
		self.local_issue_state_name.borrow().as_str()
			== self.workflow.frontmatter().tracker().in_progress_state()
	}
}
