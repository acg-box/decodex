use crate::agent::tracker_tool_bridge::tests::{self, LocalRepoDetails};

pub(in crate::agent::tracker_tool_bridge::tests) fn sample_dirty_local_repo() -> LocalRepoDetails {
	let mut local_repo = tests::sample_local_repo();

	local_repo.review_blocking_changes = vec![
		String::from("M apps/decodex/src/agent/tracker_tool_bridge/tools.rs"),
		String::from("?? apps/decodex/src/agent/new_review_surface.rs"),
	];

	local_repo
}
