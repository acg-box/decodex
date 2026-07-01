use std::{process, sync::OnceLock};

#[cfg(target_os = "linux")] use std::fs;
#[cfg(target_os = "macos")] use std::mem::{self, MaybeUninit};
#[cfg(target_os = "macos")] use std::ptr;

#[cfg(target_os = "macos")] use libc::{PROC_PIDTBSDINFO, c_char, c_void, proc_bsdinfo};

use super::RunActivityMarkerRecord;

pub(crate) fn current_host_boot_id() -> Option<String> {
	static CURRENT_HOST_BOOT_ID: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_HOST_BOOT_ID.get_or_init(read_current_host_boot_id).clone()
}

pub(crate) fn current_process_start_identity() -> Option<String> {
	static CURRENT_PROCESS_START_IDENTITY: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_PROCESS_START_IDENTITY.get_or_init(|| process_start_identity(process::id())).clone()
}

pub(crate) fn process_start_identity(process_id: u32) -> Option<String> {
	read_platform_process_start_identity(process_id)
		.and_then(|identity| normalized_process_start_identity(&identity))
}

pub(super) fn set_run_activity_marker_process_identity(
	marker: &mut RunActivityMarkerRecord,
	process_id: u32,
) {
	marker.process_id = Some(process_id);
	marker.host_boot_id = current_host_boot_id();
	marker.process_start_identity = if process_id == process::id() {
		current_process_start_identity()
	} else {
		process_start_identity(process_id)
	};
}

pub(super) fn ensure_run_activity_marker_current_process_identity(
	marker: &mut RunActivityMarkerRecord,
) {
	let current_process_id = process::id();

	match marker.process_id {
		None => set_run_activity_marker_process_identity(marker, current_process_id),
		Some(process_id)
			if process_id == current_process_id
				&& (marker.host_boot_id.is_none() || marker.process_start_identity.is_none()) =>
		{
			if marker.host_boot_id.is_none() {
				marker.host_boot_id = current_host_boot_id();
			}
			if marker.process_start_identity.is_none() {
				marker.process_start_identity = current_process_start_identity();
			}
		},
		Some(_) => {},
	}
}

fn read_current_host_boot_id() -> Option<String> {
	read_platform_host_boot_id().and_then(|boot_id| normalized_host_boot_id(&boot_id))
}

#[cfg(target_os = "linux")]
fn read_platform_host_boot_id() -> Option<String> {
	fs::read_to_string("/proc/sys/kernel/random/boot_id")
		.ok()
		.map(|boot_id| format!("linux:{boot_id}"))
}

#[cfg(target_os = "macos")]
fn read_platform_host_boot_id() -> Option<String> {
	const BOOT_SESSION_UUID_SYSCTL: &[u8] = b"kern.bootsessionuuid\0";

	let mut size = 0_usize;
	let query_status = unsafe {
		libc::sysctlbyname(
			BOOT_SESSION_UUID_SYSCTL.as_ptr().cast::<c_char>(),
			ptr::null_mut(),
			&mut size,
			ptr::null_mut(),
			0,
		)
	};

	if query_status != 0 || size == 0 {
		return None;
	}

	let mut boot_id = vec![0_u8; size];
	let read_status = unsafe {
		libc::sysctlbyname(
			BOOT_SESSION_UUID_SYSCTL.as_ptr().cast::<c_char>(),
			boot_id.as_mut_ptr().cast::<c_void>(),
			&mut size,
			ptr::null_mut(),
			0,
		)
	};

	if read_status != 0 || size == 0 {
		return None;
	}

	boot_id.truncate(size);

	if boot_id.last() == Some(&0) {
		boot_id.pop();
	}

	String::from_utf8(boot_id).ok().map(|boot_id| format!("macos_bootsessionuuid:{boot_id}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_host_boot_id() -> Option<String> {
	None
}

fn normalized_host_boot_id(boot_id: &str) -> Option<String> {
	let normalized = boot_id.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
}

#[cfg(target_os = "linux")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let stat = fs::read_to_string(format!("/proc/{process_id}/stat")).ok()?;
	let comm_end = stat.rfind(')')?;
	let after_comm = stat.get(comm_end + 2..)?;
	let start_time = after_comm.split_whitespace().nth(19)?;

	Some(format!("linux_starttime:{start_time}"))
}

#[cfg(target_os = "macos")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let Ok(pid) = i32::try_from(process_id) else {
		return None;
	};

	if pid <= 0 {
		return None;
	}

	let mut info = MaybeUninit::<proc_bsdinfo>::zeroed();
	let Ok(info_size) = i32::try_from(mem::size_of::<proc_bsdinfo>()) else {
		return None;
	};
	let read_size = unsafe {
		libc::proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast::<c_void>(), info_size)
	};

	if read_size != info_size {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(format!("macos_starttime:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_process_start_identity(_process_id: u32) -> Option<String> {
	None
}

fn normalized_process_start_identity(identity: &str) -> Option<String> {
	let normalized = identity.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
}
