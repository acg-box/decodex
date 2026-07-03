use crate::orchestrator::types::{
	Duration, IssueDispatchMode, RetainedReviewRunIdentity, RunSummary, TrackerIssue,
};

pub(crate) enum RetryDispatchDecision {
	Blocked { excluded_issue_ids: Vec<String> },
	Dispatch(Box<RunSummary>),
	Continue,
}

#[derive(Clone, Debug)]
pub(crate) enum RunLeaseDisposition {
	RetainedReviewComplete,
	Superseded { newer_run_id: String, newer_attempt_number: i64 },
	Terminal,
	NotDispatchable,
	Stalled { idle_for: Duration },
	StalledRetainedPartialProgress { idle_for: Duration },
	StalledAlreadyNeedsAttention { idle_for: Duration },
}

pub(crate) struct SelectedIssueRunCandidate {
	pub(crate) issue: TrackerIssue,
	pub(crate) dispatch_mode: IssueDispatchMode,
	pub(crate) preferred_run_identity: Option<RetainedReviewRunIdentity>,
	pub(crate) program_dispatch: Option<ProgramDispatchSelection>,
}
impl SelectedIssueRunCandidate {
	pub(crate) fn new(issue: TrackerIssue, dispatch_mode: IssueDispatchMode) -> Self {
		Self { issue, dispatch_mode, preferred_run_identity: None, program_dispatch: None }
	}

	pub(crate) fn with_program_dispatch(
		mut self,
		program_dispatch: ProgramDispatchSelection,
	) -> Self {
		self.program_dispatch = Some(program_dispatch);

		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramDispatchSelection {
	pub(crate) program_id: String,
	pub(crate) node_id: String,
	pub(crate) source_contract_id: Option<String>,
	pub(crate) queue_intent: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RetryIssueStateHint<'a> {
	pub(crate) preferred_issue_state: Option<&'a str>,
	pub(crate) preferred_initial_issue_state: Option<&'a str>,
}
