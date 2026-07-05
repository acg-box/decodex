use std::{
	fs::File,
	io::{Result, Write as _},
	path::Path,
};

use tempfile::TempDir;

use crate::state::StateStore;

struct TestFile;
impl TestFile {
	fn write(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> Result<()> {
		let mut file = File::create(path)?;

		file.write_all(body.as_ref())
	}
}

#[test]
fn read_only_shared_claim_check_does_not_remove_unlocked_claim_anchor() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-101.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("store should configure dispatch slot root");

	TestFile::write(&issue_claim_path, "stale claim anchor\n")
		.expect("stale claim anchor should write");

	assert!(
		!store
			.issue_has_active_shared_claim_read_only("pubfi", "PUB-101")
			.expect("read-only shared claim check should read")
	);
	assert!(
		issue_claim_path.exists(),
		"read-only shared claim check should not remove an unlocked claim anchor"
	);
	assert!(
		!store
			.issue_has_active_shared_claim("pubfi", "PUB-101")
			.expect("normal shared claim check should read")
	);
	assert!(
		!issue_claim_path.exists(),
		"normal shared claim check should clean an unlocked claim anchor"
	);
}
