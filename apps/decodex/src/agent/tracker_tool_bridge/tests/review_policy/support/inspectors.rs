use crate::agent::tracker_tool_bridge::{
	PullRequestDetails,
	tests::{FakeLocalRepoInspector, FakePullRequestInspector, LocalRepoDetails},
};

pub(in crate::agent::tracker_tool_bridge::tests) fn sample_review_repair_apply_inspectors(
	pr_url: &str,
) -> (FakePullRequestInspector, FakeLocalRepoInspector) {
	let inspector = FakePullRequestInspector::new(vec![
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
		Ok(PullRequestDetails {
			head_ref_name: String::from("x/decodex-pub-618"),
			head_ref_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_repository_name: String::from("decodex"),
			head_repository_owner: String::from("hack-ink"),
			is_draft: false,
			state: String::from("OPEN"),
			base_ref_name: String::from("main"),
			url: String::from(pr_url),
		}),
	]);
	let local_repo_inspector = FakeLocalRepoInspector::new(vec![
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
		Ok(LocalRepoDetails {
			default_branch: String::from("main"),
			head_oid: String::from("18a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			head_tree_oid: String::from("f8a20f7dfb9526e7421a5f095b1c6adec84e52d6"),
			repository_name: String::from("decodex"),
			repository_owner: String::from("hack-ink"),
			review_blocking_changes: Vec::new(),
		}),
	]);

	(inspector, local_repo_inspector)
}
