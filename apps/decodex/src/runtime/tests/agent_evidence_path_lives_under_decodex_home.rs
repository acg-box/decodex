use tempfile::TempDir;

use crate::runtime::{self, tests};

#[test]
fn agent_evidence_path_lives_under_decodex_home() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let _home_guard = tests::set_test_home(temp_dir.path());

	assert_eq!(
		runtime::agent_evidence_dir().expect("agent evidence path should resolve"),
		temp_dir.path().join(".codex/decodex/agent-evidence")
	);
}
