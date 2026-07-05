use std::path::Path;

use crate::manual::{self};

#[test]
fn issue_identifier_helpers_recognize_lane_directory_names() {
	let inferred =
		manual::infer_issue_identifier_from_worktree_root(Path::new("/tmp/.worktrees/XY-225"))
			.expect("issue identifier should infer from worktree basename");

	assert_eq!(inferred, "XY-225");
	assert!(!manual::looks_like_issue_identifier("decodex"));
	assert!(!manual::looks_like_issue_identifier("feature-branch"));
	assert!(manual::looks_like_issue_identifier("xy-225"));
}
