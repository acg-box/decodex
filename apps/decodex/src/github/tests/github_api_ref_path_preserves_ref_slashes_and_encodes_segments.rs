use crate::github;

#[test]
fn github_api_ref_path_preserves_ref_slashes_and_encodes_segments() {
	assert_eq!(github::github_api_ref_path("y/decodex XY-235"), "y/decodex%20XY-235");
}
