//! Process and thread liveness predicates for stale-active recovery.

use libc::pid_t;

use crate::state::{self, RunActivityMarker};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StaleActiveProcessLiveness {
	Alive,
	NotAlive,
	Unknown,
}

pub(super) fn stale_active_optional_marker_process_liveness(
	marker: Option<&RunActivityMarker>,
) -> StaleActiveProcessLiveness {
	marker.map(stale_active_marker_process_liveness).unwrap_or(StaleActiveProcessLiveness::Unknown)
}

pub(super) fn stale_active_marker_thread_active(marker: &RunActivityMarker) -> bool {
	matches!(marker.thread_status(), Some("active")) || !marker.thread_active_flags().is_empty()
}

fn stale_active_marker_process_liveness(marker: &RunActivityMarker) -> StaleActiveProcessLiveness {
	let Some(process_id) = marker.process_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if !stale_active_process_is_alive(process_id) {
		return StaleActiveProcessLiveness::NotAlive;
	}

	let Some(marker_host_boot_id) = marker.host_boot_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};
	let Some(current_host_boot_id) = state::current_host_boot_id() else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if marker_host_boot_id != current_host_boot_id.as_str() {
		return StaleActiveProcessLiveness::NotAlive;
	}

	let Some(marker_process_start_identity) = marker.process_start_identity() else {
		return StaleActiveProcessLiveness::Unknown;
	};
	let Some(current_process_start_identity) = state::process_start_identity(process_id) else {
		return StaleActiveProcessLiveness::Unknown;
	};

	if marker_process_start_identity == current_process_start_identity.as_str() {
		StaleActiveProcessLiveness::Alive
	} else {
		StaleActiveProcessLiveness::NotAlive
	}
}

#[cfg(unix)]
fn stale_active_process_is_alive(process_id: u32) -> bool {
	let Ok(process_id) = pid_t::try_from(process_id) else {
		return false;
	};

	if process_id <= 0 {
		return false;
	}

	match unsafe { libc::kill(process_id, 0) } {
		0 => true,
		-1 => matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM)),
		_ => false,
	}
}

#[cfg(not(unix))]
fn stale_active_process_is_alive(_process_id: u32) -> bool {
	false
}
