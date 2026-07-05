use std::path::PathBuf;

use crate::runtime;

#[test]
fn runtime_paths_live_under_codex_decodex_home() {
	let home = PathBuf::from("/tmp/decodex-home-test");

	assert_eq!(
		runtime::decodex_home_dir_from(home),
		PathBuf::from("/tmp/decodex-home-test/.codex/decodex")
	);
}
