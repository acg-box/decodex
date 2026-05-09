#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PullRequestLandingState {
	pub(crate) url: String,
	pub(crate) state: String,
	pub(crate) is_draft: bool,
	pub(crate) review_decision: Option<String>,
	pub(crate) base_ref_name: String,
	pub(crate) pending_review_requests: usize,
	pub(crate) mergeable: String,
	pub(crate) merge_state_status: String,
	pub(crate) head_ref_name: String,
	pub(crate) head_ref_oid: String,
	pub(crate) status_check_rollup_state: Option<String>,
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
			unresolved_review_threads: self.unresolved_review_threads,
		}
	}
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
	pub(crate) unresolved_review_threads: usize,
}

pub(crate) fn manual_landing_gates_satisfied(view: PullRequestLandingGateView<'_>) -> bool {
	view.state == "OPEN"
		&& !view.is_draft
		&& view.pending_review_requests == 0
		&& view.unresolved_review_threads == 0
		&& view.review_decision != Some("CHANGES_REQUESTED")
		&& view.mergeable == "MERGEABLE"
		&& merge_state_allows_ready_to_land(view.merge_state_status)
		&& !checks_require_wait(view.status_check_rollup_state)
		&& !failed_checks_require_repair(view.status_check_rollup_state, view.merge_state_status)
		&& merge_state_requires_review_repair(view.mergeable, view.merge_state_status).is_none()
}

pub(crate) fn retained_landing_gates_satisfied(view: PullRequestLandingGateView<'_>) -> bool {
	view.state == "OPEN"
		&& !view.is_draft
		&& view.review_decision != Some("CHANGES_REQUESTED")
		&& view.mergeable == "MERGEABLE"
		&& merge_state_allows_ready_to_land(view.merge_state_status)
		&& !checks_require_wait(view.status_check_rollup_state)
		&& !failed_checks_require_repair(view.status_check_rollup_state, view.merge_state_status)
		&& merge_state_requires_review_repair(view.mergeable, view.merge_state_status).is_none()
}

pub(crate) fn retained_clean_path_landing_gates_satisfied(
	view: PullRequestLandingGateView<'_>,
) -> bool {
	retained_landing_gates_satisfied(view)
		&& view.merge_state_status == "CLEAN"
		&& matches!(view.status_check_rollup_state, None | Some("SUCCESS"))
}

pub(crate) fn retained_landing_requires_agent_fallback(
	view: PullRequestLandingGateView<'_>,
) -> bool {
	let review_and_check_gates_ready = view.state == "OPEN"
		&& !view.is_draft
		&& view.review_decision != Some("CHANGES_REQUESTED")
		&& view.unresolved_review_threads == 0
		&& !checks_require_wait(view.status_check_rollup_state)
		&& !failed_checks_require_repair(view.status_check_rollup_state, view.merge_state_status);

	review_and_check_gates_ready
		&& ((retained_landing_gates_satisfied(view)
			&& !retained_clean_path_landing_gates_satisfied(view))
			|| view.mergeable == "UNKNOWN"
			|| view.merge_state_status == "UNKNOWN")
}

pub(crate) fn merge_state_allows_ready_to_land(merge_state_status: &str) -> bool {
	matches!(merge_state_status, "CLEAN" | "HAS_HOOKS" | "UNSTABLE")
}

pub(crate) fn checks_require_wait(check_state: Option<&str>) -> bool {
	matches!(check_state, Some("EXPECTED" | "PENDING"))
}

pub(crate) fn failed_checks_require_repair(
	check_state: Option<&str>,
	merge_state_status: &str,
) -> bool {
	matches!(check_state, Some("ERROR" | "FAILURE")) && merge_state_status == "BLOCKED"
}

pub(crate) fn merge_state_requires_review_repair(
	mergeable: &str,
	merge_state_status: &str,
) -> Option<&'static str> {
	if mergeable == "CONFLICTING" {
		return Some("pull_request_merge_conflict");
	}
	if merge_state_status == "BEHIND" {
		return Some("pull_request_branch_behind_base");
	}

	None
}

#[cfg(test)]
mod tests {
	use crate::pull_request::{self, PullRequestLandingGateView};

	fn sample_gate_view() -> PullRequestLandingGateView<'static> {
		PullRequestLandingGateView {
			state: "OPEN",
			is_draft: false,
			review_decision: Some("APPROVED"),
			pending_review_requests: 0,
			mergeable: "MERGEABLE",
			merge_state_status: "CLEAN",
			status_check_rollup_state: Some("SUCCESS"),
			unresolved_review_threads: 0,
		}
	}

	#[test]
	fn landing_gates_handle_green_pending_and_review_request_cases() {
		assert!(pull_request::manual_landing_gates_satisfied(sample_gate_view()));

		let mut view = sample_gate_view();

		view.status_check_rollup_state = Some("PENDING");

		assert!(!pull_request::manual_landing_gates_satisfied(view));
		assert!(pull_request::checks_require_wait(Some("PENDING")));

		let mut view = sample_gate_view();

		view.pending_review_requests = 2;

		assert!(pull_request::retained_landing_gates_satisfied(view));
		assert!(!pull_request::manual_landing_gates_satisfied(view));
	}

	#[test]
	fn clean_path_landing_gates_only_allow_current_green_prs() {
		assert!(pull_request::retained_clean_path_landing_gates_satisfied(sample_gate_view()));

		let mut view = sample_gate_view();

		view.merge_state_status = "HAS_HOOKS";

		assert!(pull_request::retained_landing_gates_satisfied(view));
		assert!(pull_request::retained_landing_requires_agent_fallback(view));
		assert!(!pull_request::retained_clean_path_landing_gates_satisfied(view));

		let mut view = sample_gate_view();

		view.merge_state_status = "UNSTABLE";
		view.status_check_rollup_state = Some("FAILURE");

		assert!(pull_request::retained_landing_gates_satisfied(view));
		assert!(pull_request::retained_landing_requires_agent_fallback(view));
		assert!(!pull_request::retained_clean_path_landing_gates_satisfied(view));

		let mut view = sample_gate_view();

		view.mergeable = "UNKNOWN";

		assert!(!pull_request::retained_landing_gates_satisfied(view));
		assert!(pull_request::retained_landing_requires_agent_fallback(view));

		let mut view = sample_gate_view();

		view.merge_state_status = "UNKNOWN";

		assert!(!pull_request::retained_landing_gates_satisfied(view));
		assert!(pull_request::retained_landing_requires_agent_fallback(view));
	}

	#[test]
	fn landing_gates_route_conflicts_and_branch_lag_to_repair() {
		assert_eq!(
			pull_request::merge_state_requires_review_repair("CONFLICTING", "CLEAN"),
			Some("pull_request_merge_conflict")
		);
		assert_eq!(
			pull_request::merge_state_requires_review_repair("MERGEABLE", "BEHIND"),
			Some("pull_request_branch_behind_base")
		);
	}

	#[test]
	fn merge_state_allows_ready_to_land_matches_existing_runtime_policy() {
		assert!(pull_request::merge_state_allows_ready_to_land("CLEAN"));
		assert!(pull_request::merge_state_allows_ready_to_land("HAS_HOOKS"));
		assert!(pull_request::merge_state_allows_ready_to_land("UNSTABLE"));
		assert!(!pull_request::merge_state_allows_ready_to_land("BLOCKED"));
	}

	#[test]
	fn failed_checks_require_repair_only_for_blocked_red_checks() {
		assert!(pull_request::failed_checks_require_repair(Some("FAILURE"), "BLOCKED"));
		assert!(!pull_request::failed_checks_require_repair(Some("FAILURE"), "CLEAN"));
	}
}
