//! Retry denied group signals against current members while the caller owns the unreaped leader.

use std::io;

pub(super) fn signal_group(
	group: i32,
	signal: i32,
	mut signal_target: impl FnMut(i32, i32) -> io::Result<()>,
) -> io::Result<()> {
	let group_error = match signal_target(-group, signal) {
		Err(error) if error.kind() == io::ErrorKind::PermissionDenied => error,
		result => return result,
	};
	let mut members = group_members(group)?;
	// Signal the leader last so it can reap children that exit during cleanup.
	members.sort_unstable_by_key(|pid| *pid == group);
	let mut delivered = false;
	for pid in members {
		// SAFETY: getpgid only queries membership. Recheck the listing before each signal.
		if pid > 1 && unsafe { libc::getpgid(pid) } == group {
			delivered |= signal_target(pid, signal).is_ok();
		}
	}
	// Delivery is not death evidence. The supervisor still requires an exact exit and
	// group quiescence; partial delivery must permit its bounded SIGKILL escalation.
	if delivered { Ok(()) } else { Err(group_error) }
}

fn group_members(group: i32) -> io::Result<Vec<i32>> {
	let mut members = vec![0; 16];
	loop {
		let bytes = i32::try_from(std::mem::size_of_val(members.as_slice()))
			.map_err(|_| io::Error::other("process group is too large"))?;
		// SAFETY: the buffer is writable for the supplied byte count.
		let count = unsafe { libc::proc_listpgrppids(group, members.as_mut_ptr().cast(), bytes) };
		let count = usize::try_from(count).map_err(|_| io::Error::last_os_error())?;
		if count < members.len() {
			members.truncate(count);
			return Ok(members);
		}
		let capacity = members
			.len()
			.checked_mul(2)
			.ok_or_else(|| io::Error::other("process group is too large"))?;
		members.resize(capacity, 0);
	}
}

#[cfg(test)]
#[path = "process_platform_macos_signal_tests.rs"]
mod tests;
