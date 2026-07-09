#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestLandingState {
	pub(crate) url: String,
	pub(crate) state: String,
	pub(crate) is_draft: bool,
	pub(crate) review_decision: Option<String>,
	pub(crate) base_ref_name: String,
	pub(crate) base_ref_oid: Option<String>,
	pub(crate) pending_review_requests: usize,
	pub(crate) mergeable: String,
	pub(crate) merge_state_status: String,
	pub(crate) head_ref_name: String,
	pub(crate) head_ref_oid: String,
	pub(crate) status_check_rollup_state: Option<String>,
	pub(crate) required_status_contexts: Vec<PullRequestRequiredStatusContext>,
	pub(crate) unresolved_review_threads: usize,
}
impl PullRequestLandingState {
	pub(crate) fn gate_view(&self) -> PullRequestLandingGateView<'_> {
		PullRequestLandingGateView {
			state: self.state.as_str(),
			is_draft: self.is_draft,
			review_decision: self.review_decision.as_deref(),
			pending_review_requests: self.pending_review_requests,
			mergeable: self.mergeable.as_str(),
			merge_state_status: self.merge_state_status.as_str(),
			status_check_rollup_state: self.status_check_rollup_state.as_deref(),
			fast_landing: !self.required_status_contexts.is_empty(),
			required_status_contexts: &self.required_status_contexts,
			unresolved_review_threads: self.unresolved_review_threads,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestRequiredStatusContext {
	pub(crate) context: String,
	pub(crate) state: Option<String>,
	pub(crate) creator_login: Option<String>,
	pub(crate) allowed_creator: bool,
	pub(crate) base_ref_oid: Option<String>,
	pub(crate) base_ref_matches: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PullRequestLandingGateView<'a> {
	pub(crate) state: &'a str,
	pub(crate) is_draft: bool,
	pub(crate) review_decision: Option<&'a str>,
	pub(crate) pending_review_requests: usize,
	pub(crate) mergeable: &'a str,
	pub(crate) merge_state_status: &'a str,
	pub(crate) status_check_rollup_state: Option<&'a str>,
	pub(crate) fast_landing: bool,
	pub(crate) required_status_contexts: &'a [PullRequestRequiredStatusContext],
	pub(crate) unresolved_review_threads: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LandingGateMode {
	ManualLand,
	Adopt,
	Retained,
}
impl LandingGateMode {
	pub(super) fn allows_closeout_only(self) -> bool {
		matches!(self, Self::ManualLand)
	}

	pub(super) fn requires_review_requests_clear(self) -> bool {
		matches!(self, Self::ManualLand | Self::Adopt)
	}

	pub(super) fn requires_review_threads_clear(self) -> bool {
		matches!(self, Self::ManualLand | Self::Adopt)
	}

	pub(super) fn requires_green_status_rollup(self) -> bool {
		matches!(self, Self::ManualLand | Self::Adopt)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LandingGateDecision {
	Satisfied,
	CloseoutOnly,
	Wait(&'static str),
	Repair(&'static str),
	Block(&'static str),
}
