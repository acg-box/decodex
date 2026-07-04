//! Review-handoff recovery policy checks.

use crate::{
	prelude::{Result, eyre},
	pull_request::{self, LandingGateMode, PullRequestLandingGateView, PullRequestLandingState},
	tracker::TrackerIssue,
	workflow::WorkflowTracker,
};

#[derive(Debug)]
pub(super) struct RebindSuccessStateTransition {
	pub(super) state_name: String,
	pub(super) state_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RebindMode {
	RestoreMissingHandoff,
	RestoreMissingHandoffAfterWritebackFailure,
	RefreshExistingHandoff,
	CompleteExistingHandoffState,
}
impl RebindMode {
	pub(super) fn as_str(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "restore_missing_handoff",
			Self::RestoreMissingHandoffAfterWritebackFailure =>
				"restore_missing_handoff_after_writeback_failure",
			Self::RefreshExistingHandoff => "refresh_existing_handoff",
			Self::CompleteExistingHandoffState => "complete_existing_handoff_state",
		}
	}

	pub(super) fn allows_failure_state_drift_repair(self) -> bool {
		matches!(
			self,
			Self::RestoreMissingHandoffAfterWritebackFailure | Self::CompleteExistingHandoffState
		)
	}

	pub(super) fn allows_partial_handoff_state_completion(self) -> bool {
		matches!(
			self,
			Self::RestoreMissingHandoff
				| Self::RestoreMissingHandoffAfterWritebackFailure
				| Self::CompleteExistingHandoffState
		)
	}

	pub(super) fn evidence_value(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff => "absent",
			Self::RestoreMissingHandoffAfterWritebackFailure => "absent_after_writeback_failure",
			Self::RefreshExistingHandoff => "refreshed",
			Self::CompleteExistingHandoffState => "current_state_transition",
		}
	}

	pub(super) fn summary_action(self) -> &'static str {
		match self {
			Self::RestoreMissingHandoff | Self::RestoreMissingHandoffAfterWritebackFailure =>
				"restored retained review lifecycle record",
			Self::RefreshExistingHandoff => "refreshed retained review lifecycle record",
			Self::CompleteExistingHandoffState => "completed retained review handoff state",
		}
	}
}

pub(super) fn validate_rebind_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
	mode: RebindMode,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if mode.allows_partial_handoff_state_completion()
		&& issue.state.name == tracker_policy.in_progress_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}
	if mode.allows_failure_state_drift_repair()
		&& issue.state.name == tracker_policy.failure_state()
	{
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but review handoff rebind requires `{}`{}.",
		issue.identifier,
		issue.state.name,
		success_state,
		if mode.allows_partial_handoff_state_completion() {
			format!(
				" or `{}`{} for a partial handoff recovery",
				tracker_policy.in_progress_state(),
				if mode.allows_failure_state_drift_repair() {
					format!(" or `{}` for state drift recovery", tracker_policy.failure_state())
				} else {
					String::new()
				}
			)
		} else {
			String::new()
		}
	)
}

pub(super) fn validate_adopt_issue_state_for_policy(
	tracker_policy: &WorkflowTracker,
	issue: &TrackerIssue,
) -> Result<Option<RebindSuccessStateTransition>> {
	let success_state = tracker_policy.success_state();

	if issue.state.name == success_state {
		return Ok(None);
	}
	if issue.state.name == tracker_policy.in_progress_state() {
		let state_id = issue.state_id_for_name(success_state).ok_or_else(|| {
			eyre::eyre!("State `{success_state}` was not found for issue `{}`.", issue.identifier)
		})?;

		return Ok(Some(RebindSuccessStateTransition {
			state_name: success_state.to_owned(),
			state_id: state_id.to_owned(),
		}));
	}

	eyre::bail!(
		"Issue `{}` is in `{}`, but manual takeover adopt requires `{}` or `{}`.",
		issue.identifier,
		issue.state.name,
		tracker_policy.in_progress_state(),
		success_state
	)
}

pub(super) fn validate_adopt_landing_state(landing_state: &PullRequestLandingState) -> Result<()> {
	let pr_url = super::landing_url(landing_state);
	let gate_view = landing_state.gate_view();
	let decision = pull_request::classify_landing_gate(gate_view, LandingGateMode::Adopt);

	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(()),
		decision => adopt_landing_gate_error(decision, gate_view, pr_url),
	}
}

fn adopt_landing_gate_error(
	decision: pull_request::LandingGateDecision,
	gate_view: PullRequestLandingGateView<'_>,
	pr_url: &str,
) -> Result<()> {
	match decision {
		pull_request::LandingGateDecision::Satisfied => Ok(()),
		pull_request::LandingGateDecision::CloseoutOnly
		| pull_request::LandingGateDecision::Block("pull_request_not_open") => {
			eyre::bail!("Pull request `{pr_url}` is `{}`; adopt requires `OPEN`.", gate_view.state)
		},
		pull_request::LandingGateDecision::Block("pull_request_is_draft") => {
			eyre::bail!("Pull request `{pr_url}` is still draft.")
		},
		pull_request::LandingGateDecision::Wait("pending_review_requests") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} pending review request(s).",
				gate_view.pending_review_requests
			)
		},
		pull_request::LandingGateDecision::Repair("unresolved_review_threads") => {
			eyre::bail!(
				"Pull request `{pr_url}` still has {} unresolved review thread(s).",
				gate_view.unresolved_review_threads
			)
		},
		pull_request::LandingGateDecision::Repair("review_changes_requested") => {
			eyre::bail!("Pull request `{pr_url}` still has active change requests.")
		},
		pull_request::LandingGateDecision::Repair(reason)
			if matches!(
				reason,
				"pull_request_merge_conflict" | "pull_request_branch_behind_base"
			) =>
		{
			eyre::bail!("Pull request `{pr_url}` requires review repair: {reason}.")
		},
		pull_request::LandingGateDecision::Repair("required_checks_failed") => {
			eyre::bail!("Pull request `{pr_url}` has failed required checks that need repair.")
		},
		pull_request::LandingGateDecision::Wait("checks_waiting") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` is still waiting on checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait("mergeability_unknown") => {
			eyre::bail!("Pull request `{pr_url}` mergeability is still unknown.")
		},
		pull_request::LandingGateDecision::Block("merge_state_not_ready") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not ready to adopt: mergeStateStatus=`{}`.",
				gate_view.merge_state_status
			)
		},
		pull_request::LandingGateDecision::Block("not_mergeable") => {
			eyre::bail!(
				"Pull request `{pr_url}` is not mergeable: mergeable=`{}`.",
				gate_view.mergeable
			)
		},
		pull_request::LandingGateDecision::Wait("checks_non_green") => {
			let check_state = gate_view.status_check_rollup_state.unwrap_or("unknown");

			eyre::bail!(
				"Pull request `{pr_url}` still has non-green checks: statusCheckRollup=`{check_state}`."
			)
		},
		pull_request::LandingGateDecision::Wait(reason)
		| pull_request::LandingGateDecision::Repair(reason)
		| pull_request::LandingGateDecision::Block(reason) => {
			eyre::bail!("Pull request `{pr_url}` is not ready to adopt: {reason}.")
		},
	}
}
