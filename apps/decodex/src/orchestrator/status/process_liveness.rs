#[cfg(target_os = "macos")]
use std::mem;
#[cfg(target_os = "macos")]
use std::mem::MaybeUninit;
use std::time::Duration;

use libc::pid_t;
#[cfg(target_os = "macos")]
use libc::{PROC_PIDTBSDINFO, SZOMB, c_void, proc_bsdinfo};

use crate::{
	agent::{self, RUN_LEASE_IDLE_TIMEOUT},
	orchestrator::{self, MarkerProcessLiveness},
	state::{self, RunActivityMarker},
};

pub(in crate::orchestrator) fn worktree_activity_marker_is_fresh(
	marker: &RunActivityMarker,
	now_unix_epoch: i64,
) -> bool {
	marker_process_is_alive(marker)
		&& marker
			.last_activity_unix_epoch()
			.and_then(|last_activity| {
				orchestrator::observed_idle_duration(last_activity, now_unix_epoch)
			})
			.is_some_and(|idle_for| idle_for < run_activity_idle_timeout(Some(marker)))
}

pub(in crate::orchestrator) fn run_activity_idle_timeout(
	marker: Option<&RunActivityMarker>,
) -> Duration {
	agent::protocol_activity_idle_timeout(
		marker.and_then(RunActivityMarker::protocol_activity),
		RUN_LEASE_IDLE_TIMEOUT,
	)
}

pub(in crate::orchestrator) fn marker_process_is_alive(marker: &RunActivityMarker) -> bool {
	marker_process_liveness(marker).alive
}

pub(in crate::orchestrator) fn marker_process_liveness_for_marker(
	marker: &RunActivityMarker,
) -> Option<MarkerProcessLiveness> {
	marker.process_id().map(|_| marker_process_liveness(marker))
}

pub(crate) fn process_is_alive(process_id: u32) -> bool {
	let Ok(process_id) = pid_t::try_from(process_id) else {
		return false;
	};

	if process_id <= 0 {
		return false;
	}

	// Use the kernel liveness probe directly so recovery does not depend on a shell
	// builtin or `kill` binary being present on PATH.
	match unsafe { libc::kill(process_id, 0) } {
		0 => !process_is_zombie_or_uninspectable_after_signalable_probe(process_id),
		-1 => {
			matches!(std::io::Error::last_os_error().raw_os_error(), Some(libc::EPERM))
				&& !process_is_zombie(process_id)
		},
		_ => false,
	}
}

fn marker_process_liveness(marker: &RunActivityMarker) -> MarkerProcessLiveness {
	let Some(process_id) = marker.process_id() else {
		return MarkerProcessLiveness { alive: false, reason: "process_id_missing" };
	};

	if !process_is_alive(process_id) {
		return MarkerProcessLiveness { alive: false, reason: "process_stopped" };
	}

	let Some(marker_host_boot_id) = marker.host_boot_id() else {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_missing" };
	};
	let Some(current_host_boot_id) = state::current_host_boot_id() else {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_unavailable" };
	};

	if marker_host_boot_id != current_host_boot_id.as_str() {
		return MarkerProcessLiveness { alive: false, reason: "host_boot_id_mismatch" };
	}

	let Some(marker_process_start_identity) = marker.process_start_identity() else {
		return MarkerProcessLiveness { alive: false, reason: "process_start_identity_missing" };
	};
	let Some(current_process_start_identity) = state::process_start_identity(process_id) else {
		return MarkerProcessLiveness {
			alive: false,
			reason: "process_start_identity_unavailable",
		};
	};

	if marker_process_start_identity != current_process_start_identity.as_str() {
		return MarkerProcessLiveness { alive: false, reason: "process_start_identity_mismatch" };
	}

	MarkerProcessLiveness { alive: true, reason: "process_alive" }
}

fn process_is_zombie_or_uninspectable_after_signalable_probe(process_id: pid_t) -> bool {
	process_is_zombie_or_uninspectable(process_id)
}

#[cfg(not(target_os = "macos"))]
fn process_is_zombie_or_uninspectable(process_id: pid_t) -> bool {
	process_is_zombie(process_id)
}

#[cfg(target_os = "linux")]
fn process_is_zombie(process_id: pid_t) -> bool {
	let Ok(stat) = fs::read_to_string(format!("/proc/{process_id}/stat")) else {
		return false;
	};
	let Some(comm_end) = stat.rfind(')') else {
		return false;
	};
	let Some(after_comm) = stat.get(comm_end + 2..) else {
		return false;
	};

	after_comm.split_whitespace().next() == Some("Z")
}

#[cfg(target_os = "macos")]
fn process_is_zombie_or_uninspectable(process_id: pid_t) -> bool {
	match macos_process_bsd_status(process_id) {
		Some(status) => status == SZOMB,
		None => true,
	}
}

#[cfg(target_os = "macos")]
fn process_is_zombie(process_id: pid_t) -> bool {
	macos_process_bsd_status(process_id) == Some(SZOMB)
}

#[cfg(target_os = "macos")]
fn macos_process_bsd_status(process_id: pid_t) -> Option<u32> {
	if process_id <= 0 {
		return None;
	}

	let mut info = MaybeUninit::<proc_bsdinfo>::zeroed();
	let Ok(info_size) = i32::try_from(mem::size_of::<proc_bsdinfo>()) else {
		return None;
	};
	let read_size = unsafe {
		libc::proc_pidinfo(
			process_id,
			PROC_PIDTBSDINFO,
			0,
			info.as_mut_ptr().cast::<c_void>(),
			info_size,
		)
	};

	if read_size != info_size {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(info.pbi_status)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn process_is_zombie(_process_id: pid_t) -> bool {
	false
}
