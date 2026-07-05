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
fn configure_dispatch_slot_root_prunes_unlocked_shared_lock_files() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let stale_issue_claim_path = temp_dir.path().join(".decodex-issue-claim.PUB-999.lock");
	let stale_dispatch_slot_path = temp_dir.path().join(".decodex-dispatch-slot.0.lock");
	let store = StateStore::open_in_memory().expect("state store should open");

	TestFile::write(
		&stale_issue_claim_path,
		"project_id=pubfi\nissue_id=PUB-999\nrun_id=run-stale\nissue_state=In Progress\n",
	)
	.expect("stale issue-claim anchor should write");
	TestFile::write(&stale_dispatch_slot_path, "")
		.expect("stale dispatch-slot anchor should write");

	store
		.configure_dispatch_slot_root("pubfi", temp_dir.path())
		.expect("configuration should prune unlocked shared lock anchors");

	assert!(
		!stale_issue_claim_path.exists(),
		"configuration should remove unlocked stale issue-claim anchors"
	);
	assert!(
		!stale_dispatch_slot_path.exists(),
		"configuration should remove unlocked stale dispatch-slot anchors"
	);
}
