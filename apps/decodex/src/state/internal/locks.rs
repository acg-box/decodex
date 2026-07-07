mod cleanup;
mod coordinator;
#[cfg(unix)]
mod fd_flags;
mod paths;
mod record;

#[cfg(unix)]
pub(in crate::state) use self::fd_flags::{clear_close_on_exec, set_close_on_exec};
pub(in crate::state) use self::{
	cleanup::{prune_unlocked_shared_lock_files, remove_lock_file_if_exists},
	coordinator::{acquire_shared_lock_coordinator, lock_root_from_lock_path},
	paths::{
		dispatch_slot_lock_path, issue_claim_id_from_path, issue_claim_lock_path,
		shared_lock_coordinator_path,
	},
	record::{read_issue_claim_record, write_issue_claim_record},
};
