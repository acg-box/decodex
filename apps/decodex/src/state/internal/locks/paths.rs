#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
use std::{
	env,
	path::{Path, PathBuf},
};

use crate::state::{DISPATCH_SLOT_LOCK_FILE_PREFIX, ISSUE_CLAIM_LOCK_FILE_PREFIX};

pub(in crate::state) fn dispatch_slot_lock_path(root: &Path, slot_index: usize) -> PathBuf {
	root.join(format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}.{slot_index}.lock"))
}

pub(in crate::state) fn issue_claim_lock_path(root: &Path, issue_id: &str) -> PathBuf {
	root.join(format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}.{issue_id}.lock"))
}

pub(in crate::state) fn issue_claim_id_from_path(path: &Path) -> Option<String> {
	let file_name = path.file_name()?.to_str()?;

	file_name
		.strip_prefix(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		.and_then(|suffix| suffix.strip_suffix(".lock"))
		.map(str::to_owned)
}

pub(in crate::state) fn shared_lock_coordinator_path(root: &Path) -> PathBuf {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64;

	for byte in root.as_os_str().as_bytes() {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}

	env::temp_dir().join("decodex-shared-lock-coordinators").join(format!("{hash:016x}.lock"))
}
